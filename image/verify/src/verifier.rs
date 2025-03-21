/*++

Licensed under the Apache-2.0 license.

File Name:

    verifier.rs

Abstract:

    This file is the main implementation of Caliptra Image Verifier.

--*/

use core::num::NonZeroU32;

use crate::*;
#[cfg(all(not(test), not(feature = "no-cfi")))]
use caliptra_cfi_derive::cfi_impl_fn;
use caliptra_cfi_lib::{
    cfi_assert, cfi_assert_bool, cfi_assert_eq, cfi_assert_ge, cfi_assert_ne, cfi_launder,
};
use caliptra_drivers::*;
use caliptra_image_types::*;
use memoffset::{offset_of, span_of};
use zerocopy::{FromBytes, IntoBytes};

const ZERO_DIGEST: &ImageDigest384 = &[0u32; SHA384_DIGEST_WORD_SIZE];

/// PQC public key and signature
enum PqcKeyInfo<'a> {
    Lms(&'a ImageLmsPublicKey, &'a ImageLmsSignature),
    Mldsa(&'a ImageMldsaPubKey, &'a ImageMldsaSignature),
}

/// Header Info
struct HeaderInfo<'a> {
    vendor_ecc_pub_key_idx: u32,
    vendor_pqc_pub_key_idx: u32,
    vendor_ecc_pub_key_revocation: VendorEccPubKeyRevocation,
    vendor_ecc_info: (&'a ImageEccPubKey, &'a ImageEccSignature),
    vendor_pqc_info: PqcKeyInfo<'a>,
    vendor_pqc_pub_key_revocation: u32,
    owner_ecc_info: (&'a ImageEccPubKey, &'a ImageEccSignature),
    owner_pqc_info: PqcKeyInfo<'a>,
    owner_pub_keys_digest: ImageDigest384,
    owner_pub_keys_digest_in_fuses: bool,
}

/// TOC Info
struct TocInfo<'a> {
    len: u32,
    digest: &'a ImageDigest384,
}

/// Image Info
struct ImageInfo<'a> {
    fmc: &'a ImageTocEntry,
    runtime: &'a ImageTocEntry,
}

/// Image Verifier
pub struct ImageVerifier<Env: ImageVerificationEnv> {
    /// Verification Environment
    env: Env,
}

impl<Env: ImageVerificationEnv> ImageVerifier<Env> {
    /// Create a new instance `ImageVerifier`
    ///
    /// # Arguments
    ///
    /// * `env` - Environment
    pub fn new(env: Env) -> Self {
        Self { env }
    }

    /// Verify Caliptra image
    ///
    /// # Arguments
    ///
    /// * `manifest` - Image Manifest
    /// * `image`    - Image to verify
    /// * `reason`   - Reset Reason
    ///
    /// # Returns
    ///
    /// * `ImageVerificationInfo` - Image verification information success
    #[cfg_attr(all(not(test), not(feature = "no-cfi")), cfi_impl_fn)]
    #[inline(never)]
    pub fn verify(
        &mut self,
        manifest: &ImageManifest,
        img_bundle_sz: u32,
        reason: ResetReason,
    ) -> CaliptraResult<ImageVerificationInfo> {
        // Check if manifest has required marker
        if manifest.marker != MANIFEST_MARKER {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_MANIFEST_MARKER_MISMATCH)?;
        }

        // Check if manifest size is valid
        if manifest.size as usize != core::mem::size_of::<ImageManifest>() {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_MANIFEST_SIZE_MISMATCH)?;
        }

        // Check if the PQC key type is valid.
        let pqc_key_type = FwVerificationPqcKeyType::from_u8(manifest.pqc_key_type)
            .ok_or(CaliptraError::IMAGE_VERIFIER_ERR_PQC_KEY_TYPE_INVALID)?;

        // Check if the PQC key type matches the fuse value.
        let pqc_key_type_fuse = self.env.pqc_key_type_fuse()?;
        if pqc_key_type != pqc_key_type_fuse {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_PQC_KEY_TYPE_MISMATCH)?;
        }

        // Verify the preamble
        let preamble = &manifest.preamble;
        let header_info = self.verify_preamble(preamble, reason, pqc_key_type);
        let header_info = okref(&header_info)?;

        // Verify Header
        let header = &manifest.header;
        let toc_info = self.verify_header(header, header_info);
        let toc_info = okref(&toc_info)?;

        // Verify TOC
        let image_info = self.verify_toc(manifest, toc_info, img_bundle_sz);
        let image_info = okref(&image_info)?;

        // Verify FMC
        let fmc_info = self.verify_fmc(image_info.fmc, reason)?;

        // Verify Runtime
        let runtime_info = self.verify_runtime(image_info.runtime)?;

        self.verify_svn(manifest.header.svn)?;

        let effective_fuse_svn = self.effective_fuse_svn();

        let info = ImageVerificationInfo {
            vendor_ecc_pub_key_idx: header_info.vendor_ecc_pub_key_idx,
            vendor_pqc_pub_key_idx: header_info.vendor_pqc_pub_key_idx,
            owner_pub_keys_digest: header_info.owner_pub_keys_digest,
            owner_pub_keys_digest_in_fuses: header_info.owner_pub_keys_digest_in_fuses,
            fmc: fmc_info,
            runtime: runtime_info,
            fw_svn: manifest.header.svn,
            effective_fuse_svn,
            log_info: ImageVerificationLogInfo {
                vendor_ecc_pub_key_idx: header_info.vendor_ecc_pub_key_idx,
                fuse_vendor_ecc_pub_key_revocation: header_info.vendor_ecc_pub_key_revocation,
                fuse_vendor_pqc_pub_key_revocation: header_info.vendor_pqc_pub_key_revocation,
                vendor_pqc_pub_key_idx: header_info.vendor_pqc_pub_key_idx,
                fw_log_info: FirmwareSvnLogInfo {
                    manifest_svn: manifest.header.svn,
                    reserved: 0,
                    fuse_svn: self.env.fw_fuse_svn(),
                },
            },
            pqc_key_type,
        };

        Ok(info)
    }

    /// If an SVN check is required, verifies that the given SVN is greater than
    /// or equal to the fuse SVN.
    fn verify_svn(&mut self, fw_svn: u32) -> CaliptraResult<()> {
        if self.svn_check_required() {
            if fw_svn > MAX_FIRMWARE_SVN {
                Err(CaliptraError::IMAGE_VERIFIER_ERR_FIRMWARE_SVN_GREATER_THAN_MAX_SUPPORTED)?;
            }

            if cfi_launder(fw_svn) < self.env.fw_fuse_svn() {
                Err(CaliptraError::IMAGE_VERIFIER_ERR_FIRMWARE_SVN_LESS_THAN_FUSE)?;
            } else {
                cfi_assert_ge(fw_svn, self.env.fw_fuse_svn());
            }
        }
        Ok(())
    }

    /// Calculates the effective fuse SVN.
    ///
    /// If anti-rollback is disabled, the effective fuse-SVN is zero.
    /// Otherwise, it is the value in fuses.
    fn effective_fuse_svn(&mut self) -> u32 {
        if cfi_launder(self.env.anti_rollback_disable()) {
            cfi_assert!(self.env.anti_rollback_disable());
            0_u32
        } else {
            cfi_assert!(!self.env.anti_rollback_disable());
            self.env.fw_fuse_svn()
        }
    }

    /// Verify Preamble
    #[cfg_attr(all(not(test), not(feature = "no-cfi")), cfi_impl_fn)]
    fn verify_preamble<'a>(
        &mut self,
        preamble: &'a ImagePreamble,
        reason: ResetReason,
        pqc_key_type: FwVerificationPqcKeyType,
    ) -> CaliptraResult<HeaderInfo<'a>> {
        // Verify Vendor Public Key Info Digest
        self.verify_vendor_pub_key_info_digest(preamble, pqc_key_type)?;

        // Verify Owner Public Key Info Digest
        let (owner_pub_keys_digest, owner_pub_keys_digest_in_fuses) =
            self.verify_owner_pk_digest(reason)?;

        // Verify ECC Vendor Key Index
        let (vendor_ecc_pub_key_idx, vendor_ecc_pub_key_revocation) =
            self.verify_vendor_ecc_pk_idx(preamble, reason)?;

        // ECC Vendor Information
        let vendor_ecc_info = (
            &preamble.vendor_ecc_active_pub_key,
            &preamble.vendor_sigs.ecc_sig,
        );

        struct PubKeyIndexInfo {
            key_idx: u32,
            key_revocation: u32,
        }

        // Verify PQC Vendor Key Index
        let vendor_pqc_info: PqcKeyInfo<'a>;
        let vendor_pqc_pub_key_idx_info = match pqc_key_type {
            FwVerificationPqcKeyType::LMS => {
                // Read the LMS public key and signature from the preamble
                let (lms_pub_key, _) = ImageLmsPublicKey::ref_from_prefix(
                    preamble.vendor_pqc_active_pub_key.0.as_bytes(),
                )
                .or(Err(
                    CaliptraError::IMAGE_VERIFIER_ERR_LMS_VENDOR_PUB_KEY_INVALID,
                ))?;

                let (lms_sig, _) =
                    ImageLmsSignature::ref_from_prefix(preamble.vendor_sigs.pqc_sig.0.as_bytes())
                        .or(Err(
                        CaliptraError::IMAGE_VERIFIER_ERR_LMS_VENDOR_SIG_INVALID,
                    ))?;

                vendor_pqc_info = PqcKeyInfo::Lms(lms_pub_key, lms_sig);

                // Verify the vendor LMS public key index and revocation status
                let key_revocation = self.env.vendor_lms_pub_key_revocation();
                let vendor_pqc_pub_key_idx =
                    self.verify_vendor_pqc_pk_idx(preamble, reason, key_revocation)?;

                // Return the public key index information
                PubKeyIndexInfo {
                    key_idx: vendor_pqc_pub_key_idx,
                    key_revocation,
                }
            }
            FwVerificationPqcKeyType::MLDSA => {
                let (mldsa_pub_key, _) = ImageMldsaPubKey::ref_from_prefix(
                    preamble.vendor_pqc_active_pub_key.0.as_bytes(),
                )
                .or(Err(
                    CaliptraError::IMAGE_VERIFIER_ERR_MLDSA_VENDOR_PUB_KEY_READ_FAILED,
                ))?;

                let (mldsa_sig, _) =
                    ImageMldsaSignature::ref_from_prefix(preamble.vendor_sigs.pqc_sig.0.as_bytes())
                        .or(Err(
                            CaliptraError::IMAGE_VERIFIER_ERR_MLDSA_VENDOR_SIG_READ_FAILED,
                        ))?;

                vendor_pqc_info = PqcKeyInfo::Mldsa(mldsa_pub_key, mldsa_sig);

                // Verify the vendor MLDSA public key index and revocation status
                let key_revocation = self.env.vendor_mldsa_pub_key_revocation();
                let vendor_pqc_pub_key_idx =
                    self.verify_vendor_pqc_pk_idx(preamble, reason, key_revocation)?;

                // Return the public key index information
                PubKeyIndexInfo {
                    key_idx: vendor_pqc_pub_key_idx,
                    key_revocation,
                }
            }
        };

        // Owner Information
        let owner_ecc_info = (
            &preamble.owner_pub_keys.ecc_pub_key,
            &preamble.owner_sigs.ecc_sig,
        );

        let owner_pqc_info: PqcKeyInfo<'a> = match pqc_key_type {
            FwVerificationPqcKeyType::LMS => {
                let (lms_pub_key, _) = ImageLmsPublicKey::ref_from_prefix(
                    preamble.owner_pub_keys.pqc_pub_key.0.as_bytes(),
                )
                .or(Err(
                    CaliptraError::IMAGE_VERIFIER_ERR_LMS_OWNER_PUB_KEY_INVALID,
                ))?;

                let (lms_sig, _) =
                    ImageLmsSignature::ref_from_prefix(preamble.owner_sigs.pqc_sig.0.as_bytes())
                        .or(Err(CaliptraError::IMAGE_VERIFIER_ERR_LMS_OWNER_SIG_INVALID))?;

                PqcKeyInfo::Lms(lms_pub_key, lms_sig)
            }
            FwVerificationPqcKeyType::MLDSA => {
                let (mldsa_pub_key, _) = ImageMldsaPubKey::ref_from_prefix(
                    preamble.owner_pub_keys.pqc_pub_key.0.as_bytes(),
                )
                .or(Err(
                    CaliptraError::IMAGE_VERIFIER_ERR_MLDSA_OWNER_PUB_KEY_READ_FAILED,
                ))?;

                let (mldsa_sig, _) =
                    ImageMldsaSignature::ref_from_prefix(preamble.owner_sigs.pqc_sig.0.as_bytes())
                        .or(Err(
                            CaliptraError::IMAGE_VERIFIER_ERR_MLDSA_OWNER_SIG_READ_FAILED,
                        ))?;

                PqcKeyInfo::Mldsa(mldsa_pub_key, mldsa_sig)
            }
        };

        let info = HeaderInfo {
            vendor_ecc_pub_key_idx,
            vendor_pqc_pub_key_idx: vendor_pqc_pub_key_idx_info.key_idx,
            vendor_ecc_info,
            vendor_pqc_info,
            owner_pqc_info,
            owner_pub_keys_digest,
            owner_pub_keys_digest_in_fuses,
            owner_ecc_info,
            vendor_ecc_pub_key_revocation,
            vendor_pqc_pub_key_revocation: vendor_pqc_pub_key_idx_info.key_revocation,
        };

        Ok(info)
    }

    /// Verify Vendor ECC Public Key Index
    fn verify_vendor_ecc_pk_idx(
        &mut self,
        preamble: &ImagePreamble,
        reason: ResetReason,
    ) -> CaliptraResult<(u32, VendorEccPubKeyRevocation)> {
        let key_idx = preamble.vendor_ecc_pub_key_idx;
        let revocation = self.env.vendor_ecc_pub_key_revocation();
        let key_hash_count = preamble
            .vendor_pub_key_info
            .ecc_key_descriptor
            .key_hash_count;
        let last_key_idx: u32 = key_hash_count as u32 - 1;

        // Check if the key index is within bounds.
        if key_idx > last_key_idx {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INDEX_OUT_OF_BOUNDS)?;
        }

        // Check if key idx is the last key index. Last key index is never revoked.
        if key_idx == last_key_idx {
            cfi_assert_eq(cfi_launder(key_idx), last_key_idx);
        } else {
            let key = VendorEccPubKeyRevocation::from_bits_truncate(0x01u32 << key_idx);
            if cfi_launder(revocation).contains(cfi_launder(key)) {
                Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_REVOKED)?;
            } else {
                cfi_assert!(!revocation.contains(key));
            }
        }

        if cfi_launder(reason) == ResetReason::UpdateReset {
            let expected = self.env.vendor_ecc_pub_key_idx_dv();
            if cfi_launder(expected) != key_idx {
                Err(
                    CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_VENDOR_ECC_PUB_KEY_IDX_MISMATCH,
                )?;
            } else {
                cfi_assert_eq(self.env.vendor_ecc_pub_key_idx_dv(), key_idx);
            }
        } else {
            cfi_assert_ne(reason, ResetReason::UpdateReset);
        }

        Ok((key_idx, revocation))
    }

    /// Verify Vendor PQC (LMS or MLDSA) Public Key Index
    fn verify_vendor_pqc_pk_idx(
        &mut self,
        preamble: &ImagePreamble,
        reason: ResetReason,
        revocation: u32,
    ) -> CaliptraResult<u32> {
        let key_idx = preamble.vendor_pqc_pub_key_idx;
        let key_hash_count = preamble
            .vendor_pub_key_info
            .pqc_key_descriptor
            .key_hash_count;
        let last_key_idx: u32 = key_hash_count as u32 - 1;

        // Check if the key index is within bounds.
        if key_idx > last_key_idx {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_OUT_OF_BOUNDS)?;
        }

        // Check if key idx is the last key index. Last key index is never revoked.
        if key_idx == last_key_idx {
            cfi_assert_eq(cfi_launder(key_idx), last_key_idx);
        } else if (cfi_launder(revocation) & (0x01u32 << key_idx)) != 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_REVOKED)?;
        } else {
            cfi_assert_eq(revocation & (0x01u32 << key_idx), 0);
        }

        if cfi_launder(reason) == ResetReason::UpdateReset {
            let expected = self.env.vendor_pqc_pub_key_idx_dv();
            if cfi_launder(expected) != key_idx {
                Err(
                    CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_VENDOR_PQC_PUB_KEY_IDX_MISMATCH,
                )?;
            } else {
                cfi_assert_eq(self.env.vendor_pqc_pub_key_idx_dv(), key_idx);
            }
        } else {
            cfi_assert_ne(reason, ResetReason::UpdateReset);
        }

        Ok(key_idx)
    }

    /// Verify vendor public key info digest
    fn verify_vendor_pub_key_info_digest(
        &mut self,
        preamble: &ImagePreamble,
        pqc_key_type: FwVerificationPqcKeyType,
    ) -> Result<(), NonZeroU32> {
        // We skip vendor public key check in unprovisioned state
        if cfi_launder(self.env.dev_lifecycle() as u32) == Lifecycle::Unprovisioned as u32 {
            cfi_assert_eq(
                self.env.dev_lifecycle() as u32,
                Lifecycle::Unprovisioned as u32,
            );
            return Ok(());
        } else {
            cfi_assert_ne(
                self.env.dev_lifecycle() as u32,
                Lifecycle::Unprovisioned as u32,
            );
        }

        // Read expected value from the fuses
        let expected = &self.env.vendor_pub_key_info_digest_fuses();

        // Vendor public key digest from the fuses must never be zero
        if cfi_launder(expected) == ZERO_DIGEST {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_INVALID)?;
        } else {
            cfi_assert_ne(expected, ZERO_DIGEST);
        }

        let pub_key_info = preamble.vendor_pub_key_info;

        // Validate the ECC key descriptor.
        if pub_key_info.ecc_key_descriptor.version != KEY_DESCRIPTOR_VERSION {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_ECC_KEY_DESCRIPTOR_VERSION_MISMATCH)?;
        }
        if pub_key_info.ecc_key_descriptor.key_hash_count == 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_ECC_KEY_DESCRIPTOR_INVALID_HASH_COUNT)?;
        }
        if pub_key_info.ecc_key_descriptor.key_hash_count > VENDOR_ECC_MAX_KEY_COUNT as u8 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_ECC_KEY_DESCRIPTOR_HASH_COUNT_GT_MAX)?;
        }

        // Validate the PQC key descriptor.
        if pub_key_info.pqc_key_descriptor.version != KEY_DESCRIPTOR_VERSION {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_PQC_KEY_DESCRIPTOR_VERSION_MISMATCH)?;
        }
        if pub_key_info.pqc_key_descriptor.key_type != pqc_key_type as u8 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_PQC_KEY_DESCRIPTOR_TYPE_MISMATCH)?;
        }
        if pub_key_info.pqc_key_descriptor.key_hash_count == 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_PQC_KEY_DESCRIPTOR_INVALID_HASH_COUNT)?;
        }
        if (pqc_key_type == FwVerificationPqcKeyType::LMS
            && pub_key_info.pqc_key_descriptor.key_hash_count > VENDOR_LMS_MAX_KEY_COUNT as u8)
            || (pqc_key_type == FwVerificationPqcKeyType::MLDSA
                && pub_key_info.pqc_key_descriptor.key_hash_count
                    > VENDOR_MLDSA_MAX_KEY_COUNT as u8)
        {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_PQC_KEY_DESCRIPTOR_HASH_COUNT_GT_MAX)?;
        }

        let range = ImageManifest::vendor_pub_key_descriptors_range();

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_VENDOR_PUB_KEY_DIGEST_FAILURE,
                caliptra_drivers::FipsTestHook::SHA384_DIGEST_FAILURE,
            )
        };

        let actual = &self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_FAILURE
            })?;

        if cfi_launder(expected) != actual {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_MISMATCH)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(expected, actual);
        }

        self.verify_active_ecc_pub_key_digest(preamble)?;
        self.verify_active_pqc_pub_key_digest(preamble, pqc_key_type)?;

        Ok(())
    }

    fn verify_active_ecc_pub_key_digest(&mut self, preamble: &ImagePreamble) -> CaliptraResult<()> {
        let pub_key_info = preamble.vendor_pub_key_info;
        let ecc_key_idx = preamble.vendor_ecc_pub_key_idx;

        let expected = pub_key_info
            .ecc_key_descriptor
            .key_hash
            .get(ecc_key_idx as usize)
            .ok_or(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INDEX_OUT_OF_BOUNDS)?;

        let range = {
            let offset = offset_of!(ImageManifest, preamble) as u32;
            let span = span_of!(ImagePreamble, vendor_ecc_active_pub_key);
            span.start as u32 + offset..span.end as u32 + offset
        };

        let actual = &self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_FAILURE
            })?;

        if cfi_launder(expected) != actual {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_DIGEST_MISMATCH)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(expected, actual);
        }

        Ok(())
    }

    fn verify_active_pqc_pub_key_digest(
        &mut self,
        preamble: &ImagePreamble,
        pqc_key_type: FwVerificationPqcKeyType,
    ) -> CaliptraResult<()> {
        let pub_key_info = preamble.vendor_pub_key_info;
        let pqc_key_idx = preamble.vendor_pqc_pub_key_idx;

        let expected = pub_key_info
            .pqc_key_descriptor
            .key_hash
            .get(pqc_key_idx as usize);

        if expected.is_none()
            || (pqc_key_type == FwVerificationPqcKeyType::LMS
                && pqc_key_idx >= VENDOR_LMS_MAX_KEY_COUNT)
            || (pqc_key_type == FwVerificationPqcKeyType::MLDSA
                && pqc_key_idx >= VENDOR_MLDSA_MAX_KEY_COUNT)
        {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_OUT_OF_BOUNDS)?
        }

        let expected = expected.unwrap();

        let start = {
            let offset = offset_of!(ImageManifest, preamble) as u32;
            let span = span_of!(ImagePreamble, vendor_pqc_active_pub_key);
            span.start as u32 + offset
        };

        let size = if pqc_key_type == FwVerificationPqcKeyType::MLDSA {
            MLDSA87_PUB_KEY_BYTE_SIZE
        } else {
            LMS_PUB_KEY_BYTE_SIZE
        } as u32;

        let actual = &self.env.sha384_digest(start, size).map_err(|err| {
            self.env.set_fw_extended_error(err.into());
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_FAILURE
        })?;

        if cfi_launder(expected) != actual {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_DIGEST_MISMATCH)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(expected, actual);
        }

        Ok(())
    }

    /// Verify owner public key digest.
    /// Returns a bool indicating whether the digest was in fuses.
    fn verify_owner_pk_digest(
        &mut self,
        reason: ResetReason,
    ) -> CaliptraResult<(ImageDigest384, bool)> {
        let range = ImageManifest::owner_pub_key_range();

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_OWNER_PUB_KEY_DIGEST_FAILURE,
                caliptra_drivers::FipsTestHook::SHA384_DIGEST_FAILURE,
            )
        };

        let actual = &self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_FAILURE
            })?;

        let fuses_digest = &self.env.owner_pub_key_digest_fuses();

        if fuses_digest == ZERO_DIGEST {
            caliptra_cfi_lib::cfi_assert_eq_12_words(fuses_digest, ZERO_DIGEST);
        } else if fuses_digest != actual {
            return Err(CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_MISMATCH);
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(fuses_digest, actual);
        }

        if cfi_launder(reason) == ResetReason::UpdateReset {
            let cold_boot_digest = &self.env.owner_pub_key_digest_dv();
            if cfi_launder(cold_boot_digest) != actual {
                return Err(CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_OWNER_DIGEST_FAILURE);
            } else {
                caliptra_cfi_lib::cfi_assert_eq_12_words(cold_boot_digest, actual);
            }
        } else {
            cfi_assert_ne(reason, ResetReason::UpdateReset);
        }

        Ok((*actual, fuses_digest != ZERO_DIGEST))
    }

    /// Verify Header
    #[cfg_attr(all(not(test), not(feature = "no-cfi")), cfi_impl_fn)]
    fn verify_header<'a>(
        &mut self,
        header: &'a ImageHeader,
        info: &HeaderInfo,
    ) -> CaliptraResult<TocInfo<'a>> {
        // Calculate the digest for the header
        let range = ImageManifest::header_range();
        let vendor_header_len = offset_of!(ImageHeader, owner_data);

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_HEADER_DIGEST_FAILURE,
                caliptra_drivers::FipsTestHook::SHA384_DIGEST_FAILURE,
            )
        };

        // Vendor header digest is calculated up to the owner_data field.
        let vendor_digest_384 = self
            .env
            .sha384_digest(range.start, vendor_header_len as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_HEADER_DIGEST_FAILURE
            })?;

        let mut vendor_digest_holder = ImageDigestHolder {
            digest_384: &vendor_digest_384,
            digest_512: None,
        };

        let owner_digest_384 = self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_HEADER_DIGEST_FAILURE
            })?;

        let mut owner_digest_holder = ImageDigestHolder {
            digest_384: &owner_digest_384,
            digest_512: None,
        };

        let vendor_digest_512: [u32; 16];
        let owner_digest_512: [u32; 16];

        // Update vendor_digest_holder and owner_digest_holder with SHA512 digests if MLDSA validation i required.
        if let PqcKeyInfo::Mldsa(_, _) = info.vendor_pqc_info {
            vendor_digest_512 = self
                .env
                .sha512_digest(range.start, vendor_header_len as u32)
                .map_err(|err| {
                    self.env.set_fw_extended_error(err.into());
                    CaliptraError::IMAGE_VERIFIER_ERR_HEADER_DIGEST_FAILURE
                })?;
            vendor_digest_holder.digest_512 = Some(&vendor_digest_512);

            owner_digest_512 = self
                .env
                .sha512_digest(range.start, range.len() as u32)
                .map_err(|err| {
                    self.env.set_fw_extended_error(err.into());
                    CaliptraError::IMAGE_VERIFIER_ERR_HEADER_DIGEST_FAILURE
                })?;
            owner_digest_holder.digest_512 = Some(&owner_digest_512);
        }

        // Verify vendor signatures.
        self.verify_vendor_sig(
            &vendor_digest_holder,
            info.vendor_ecc_info,
            &info.vendor_pqc_info,
        )?;

        // Verify the ECC public key index used to verify header signature is encoded
        // in the header
        if cfi_launder(header.vendor_ecc_pub_key_idx) != info.vendor_ecc_pub_key_idx {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INDEX_MISMATCH)?;
        } else {
            cfi_assert_eq(header.vendor_ecc_pub_key_idx, info.vendor_ecc_pub_key_idx);
        }

        // Verify the PQC (LMS or MLDSA) public key index used to verify header signature is encoded
        // in the header
        if cfi_launder(header.vendor_pqc_pub_key_idx) != info.vendor_pqc_pub_key_idx {
            return Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_MISMATCH);
        } else {
            cfi_assert_eq(header.vendor_pqc_pub_key_idx, info.vendor_pqc_pub_key_idx);
        }

        // Verify owner signatures.
        self.verify_owner_sig(
            &owner_digest_holder,
            info.owner_ecc_info,
            &info.owner_pqc_info,
        )?;

        let verif_info = TocInfo {
            len: header.toc_len,
            digest: &header.toc_digest,
        };

        Ok(verif_info)
    }

    /// Verify Owner Signature
    // Inlined to reduce ROM size
    #[inline(always)]
    fn verify_ecc_sig(
        &mut self,
        digest: &ImageDigest384,
        pub_key: &ImageEccPubKey,
        sig: &ImageEccSignature,
        is_owner: bool,
    ) -> CaliptraResult<()> {
        let (signature_invalid, pub_key_invalid_arg, sig_invalid_arg) = if is_owner {
            (
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_SIGNATURE_INVALID,
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_PUB_KEY_INVALID_ARG,
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_SIGNATURE_INVALID_ARG,
            )
        } else {
            (
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID,
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INVALID_ARG,
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID_ARG,
            )
        };

        if &pub_key.x == ZERO_DIGEST || &pub_key.y == ZERO_DIGEST {
            return Err(pub_key_invalid_arg);
        }
        if &sig.r == ZERO_DIGEST || &sig.s == ZERO_DIGEST {
            return Err(sig_invalid_arg);
        }

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            let fips_verify_failure = if is_owner {
                caliptra_drivers::FipsTestHook::FW_LOAD_OWNER_ECC_VERIFY_FAILURE
            } else {
                caliptra_drivers::FipsTestHook::FW_LOAD_VENDOR_ECC_VERIFY_FAILURE
            };
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                fips_verify_failure,
                caliptra_drivers::FipsTestHook::ECC384_VERIFY_FAILURE,
            )
        };

        let verify_r = self
            .env
            .ecc384_verify(digest, pub_key, sig)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                if is_owner {
                    CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_VERIFY_FAILURE
                } else {
                    CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_VERIFY_FAILURE
                }
            })?;

        if cfi_launder(verify_r) != caliptra_drivers::Array4xN(sig.r) {
            return Err(signature_invalid);
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(&verify_r.0, &sig.r);
        }

        Ok(())
    }

    /// Verify Vendor Signature
    fn verify_vendor_sig(
        &mut self,
        digest_holder: &ImageDigestHolder,
        ecc_info: (&ImageEccPubKey, &ImageEccSignature),
        pqc_info: &PqcKeyInfo,
    ) -> CaliptraResult<()> {
        let (ecc_pub_key, ecc_sig) = ecc_info;
        if &ecc_pub_key.x == ZERO_DIGEST || &ecc_pub_key.y == ZERO_DIGEST {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INVALID_ARG)?;
        }
        if &ecc_sig.r == ZERO_DIGEST || &ecc_sig.s == ZERO_DIGEST {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID_ARG)?;
        }

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_VENDOR_ECC_VERIFY_FAILURE,
                caliptra_drivers::FipsTestHook::ECC384_VERIFY_FAILURE,
            )
        };

        let verify_r = self
            .env
            .ecc384_verify(digest_holder.digest_384, ecc_pub_key, ecc_sig)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_VERIFY_FAILURE
            })?;

        if cfi_launder(verify_r) != caliptra_drivers::Array4xN(ecc_sig.r) {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(&verify_r.0, &ecc_sig.r);
        }

        // Verify PQC signature.
        match pqc_info {
            PqcKeyInfo::Lms(lms_pub_key, lms_sig) => {
                self.verify_lms_sig(digest_holder.digest_384, lms_pub_key, lms_sig, false)?;
            }
            PqcKeyInfo::Mldsa(mldsa_pub_key, mldsa_sig) => {
                if let Some(digest_512) = digest_holder.digest_512 {
                    self.verify_mldsa_sig(digest_512, mldsa_pub_key, mldsa_sig, false)?;
                } else {
                    Err(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_MLDSA_DIGEST_MISSING)?;
                }
            }
        }

        Ok(())
    }

    fn verify_owner_sig(
        &mut self,
        digest_holder: &ImageDigestHolder,
        ecc_info: (&ImageEccPubKey, &ImageEccSignature),
        pqc_info: &PqcKeyInfo,
    ) -> CaliptraResult<()> {
        // Verify owner ECC signature
        let (ecc_pub_key, ecc_sig) = ecc_info;
        self.verify_ecc_sig(digest_holder.digest_384, ecc_pub_key, ecc_sig, true)?;

        // Verify owner PQC signature
        match pqc_info {
            PqcKeyInfo::Lms(lms_pub_key, lms_sig) => {
                self.verify_lms_sig(digest_holder.digest_384, lms_pub_key, lms_sig, true)?;
            }
            PqcKeyInfo::Mldsa(mldsa_pub_key, mldsa_sig) => {
                if let Some(digest_512) = digest_holder.digest_512 {
                    self.verify_mldsa_sig(digest_512, mldsa_pub_key, mldsa_sig, true)?;
                } else {
                    Err(CaliptraError::IMAGE_VERIFIER_ERR_OWNER_MLDSA_DIGEST_MISSING)?;
                }
            }
        }

        Ok(())
    }

    /// Verify owner LMS Signature
    fn verify_lms_sig(
        &mut self,
        digest: &ImageDigest384,
        lms_pub_key: &ImageLmsPublicKey,
        lms_sig: &ImageLmsSignature,
        is_owner: bool,
    ) -> CaliptraResult<()> {
        let (verify_failure, signature_invalid) = if is_owner {
            (
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_LMS_VERIFY_FAILURE,
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_LMS_SIGNATURE_INVALID,
            )
        } else {
            (
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_LMS_VERIFY_FAILURE,
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_LMS_SIGNATURE_INVALID,
            )
        };

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            let fips_verify_failure = if is_owner {
                caliptra_drivers::FipsTestHook::FW_LOAD_OWNER_LMS_VERIFY_FAILURE
            } else {
                caliptra_drivers::FipsTestHook::FW_LOAD_VENDOR_LMS_VERIFY_FAILURE
            };
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                fips_verify_failure,
                caliptra_drivers::FipsTestHook::LMS_VERIFY_FAILURE,
            )
        };

        let candidate_key = self
            .env
            .lms_verify(digest, lms_pub_key, lms_sig)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                verify_failure
            })?;

        let pub_key_digest = HashValue::from(lms_pub_key.digest);
        if candidate_key != pub_key_digest {
            return Err(signature_invalid);
        } else {
            caliptra_cfi_lib::cfi_assert_eq_6_words(&candidate_key.0, &pub_key_digest.0);
        }

        Ok(())
    }

    /// Verify MLDSA Signature
    fn verify_mldsa_sig(
        &mut self,
        digest: &ImageDigest512,
        mldsa_pub_key: &ImageMldsaPubKey,
        mldsa_sig: &ImageMldsaSignature,
        is_owner: bool,
    ) -> CaliptraResult<()> {
        let (verify_failure, signature_invalid) = if is_owner {
            (
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_MLDSA_VERIFY_FAILURE,
                CaliptraError::IMAGE_VERIFIER_ERR_OWNER_MLDSA_SIGNATURE_INVALID,
            )
        } else {
            (
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_MLDSA_VERIFY_FAILURE,
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_MLDSA_SIGNATURE_INVALID,
            )
        };

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            let fips_verify_failure = if is_owner {
                caliptra_drivers::FipsTestHook::FW_LOAD_OWNER_MLDSA_VERIFY_FAILURE
            } else {
                caliptra_drivers::FipsTestHook::FW_LOAD_VENDOR_MLDSA_VERIFY_FAILURE
            };
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                fips_verify_failure,
                caliptra_drivers::FipsTestHook::MLDSA_VERIFY_FAILURE,
            )
        };

        let result = self
            .env
            .mldsa87_verify(digest, mldsa_pub_key, mldsa_sig)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                verify_failure
            })?;

        if cfi_launder(result) != Mldsa87Result::Success {
            Err(signature_invalid)?;
        }

        Ok(())
    }

    /// Verify Table of Contents
    #[cfg_attr(all(not(test), not(feature = "no-cfi")), cfi_impl_fn)]
    fn verify_toc<'a>(
        &mut self,
        manifest: &'a ImageManifest,
        verify_info: &TocInfo,
        img_bundle_sz: u32,
    ) -> CaliptraResult<ImageInfo<'a>> {
        if cfi_launder(verify_info.len) != MAX_TOC_ENTRY_COUNT {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_TOC_ENTRY_COUNT_INVALID)?;
        } else {
            cfi_assert_eq(verify_info.len, MAX_TOC_ENTRY_COUNT);
        }

        let range = ImageManifest::toc_range();

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_OWNER_TOC_DIGEST_FAILURE,
                caliptra_drivers::FipsTestHook::SHA384_DIGEST_FAILURE,
            )
        };

        let actual = self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_TOC_DIGEST_FAILURE
            })?;

        if cfi_launder(*verify_info.digest) != actual {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_TOC_DIGEST_MISMATCH)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(verify_info.digest, &actual);
        }

        // Verify the FMC size is not zero.
        if manifest.fmc.image_size() == 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_SIZE_ZERO)?;
        }

        // Verify the Runtime size is not zero.
        if manifest.runtime.image_size() == 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_SIZE_ZERO)?;
        }

        // Image length does not exceed the Image Bundle size
        let img_len: u64 = manifest.size as u64
            + manifest.fmc.image_size() as u64
            + manifest.runtime.image_size() as u64;

        if img_len > img_bundle_sz.into() {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_IMAGE_LEN_MORE_THAN_BUNDLE_SIZE)?;
        }

        // Check if fmc and runtime sections overlap in the image.
        let fmc_range = manifest.fmc.image_range()?;
        let runtime_range = manifest.runtime.image_range()?;
        if fmc_range.start < runtime_range.end && fmc_range.end > runtime_range.start {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)?;
        }

        // Ensure the fmc section is before the runtime section in the manifest.
        if fmc_range.end > runtime_range.start {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_INCORRECT_ORDER)?;
        }

        // Check if fmc and runtime images don't overlap on loading in the ICCM.
        let fmc_load_addr_start = manifest.fmc.load_addr;
        let (fmc_load_addr_end, overflow) =
            fmc_load_addr_start.overflowing_add(manifest.fmc.image_size() - 1);
        if overflow {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDRESS_IMAGE_SIZE_ARITHMETIC_OVERFLOW)?;
        }

        let runtime_load_addr_start = manifest.runtime.load_addr;
        let (runtime_load_addr_end, overflow) =
            runtime_load_addr_start.overflowing_add(manifest.runtime.image_size() - 1);
        if overflow {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDRESS_IMAGE_SIZE_ARITHMETIC_OVERFLOW)?;
        }

        if fmc_load_addr_start <= runtime_load_addr_end
            && fmc_load_addr_end >= runtime_load_addr_start
        {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_LOAD_ADDR_OVERLAP)?;
        }

        let info = ImageInfo {
            fmc: &manifest.fmc,
            runtime: &manifest.runtime,
        };

        Ok(info)
    }

    // Check if SVN check is required
    #[inline(always)]
    fn svn_check_required(&mut self) -> bool {
        // If device is unprovisioned or if rollback is enabled (anti_rollback_disable == true), don't check the SVN.
        if cfi_launder(self.env.dev_lifecycle() as u32) == Lifecycle::Unprovisioned as u32 {
            cfi_assert_eq(
                self.env.dev_lifecycle() as u32,
                Lifecycle::Unprovisioned as u32,
            );
            false // SVN check not required
        } else if cfi_launder(self.env.anti_rollback_disable()) {
            cfi_assert!(self.env.anti_rollback_disable());
            false // SVN check not required
        } else {
            true // SVN check required
        }
    }

    /// Verify FMC
    #[cfg_attr(all(not(test), not(feature = "no-cfi")), cfi_impl_fn)]
    fn verify_fmc(
        &mut self,
        verify_info: &ImageTocEntry,
        reason: ResetReason,
    ) -> CaliptraResult<ImageVerificationExeInfo> {
        let range = verify_info.image_range()?;

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_FMC_DIGEST_FAILURE,
                caliptra_drivers::FipsTestHook::SHA384_DIGEST_FAILURE,
            )
        };

        let actual = self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_FMC_DIGEST_FAILURE
            })?;

        if cfi_launder(verify_info.digest) != actual {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_DIGEST_MISMATCH)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(&verify_info.digest, &actual);
        }

        // Overflow/underflow is checked in verify_toc
        if !self.env.iccm_range().contains(&verify_info.load_addr)
            || !self
                .env
                .iccm_range()
                .contains(&(verify_info.load_addr + verify_info.size - 1))
        {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDR_INVALID)?;
        }
        if verify_info.load_addr % 4 != 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDR_UNALIGNED)?;
        }

        if !self.env.iccm_range().contains(&verify_info.entry_point) {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_ENTRY_POINT_INVALID)?;
        }
        if verify_info.entry_point % 4 != 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_FMC_ENTRY_POINT_UNALIGNED)?;
        }

        if cfi_launder(reason) == ResetReason::UpdateReset {
            if cfi_launder(actual) != self.env.get_fmc_digest_dv() {
                Err(CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_FMC_DIGEST_MISMATCH)?;
            } else {
                cfi_assert_eq(actual, self.env.get_fmc_digest_dv());
            }
        } else {
            cfi_assert_ne(reason, ResetReason::UpdateReset);
        }

        let info = ImageVerificationExeInfo {
            load_addr: verify_info.load_addr,
            entry_point: verify_info.entry_point,
            digest: verify_info.digest,
            size: verify_info.size,
        };

        Ok(info)
    }

    /// Verify Runtime
    #[cfg_attr(all(not(test), not(feature = "no-cfi")), cfi_impl_fn)]
    fn verify_runtime(
        &mut self,
        verify_info: &ImageTocEntry,
    ) -> CaliptraResult<ImageVerificationExeInfo> {
        let range = verify_info.image_range()?;

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::update_hook_cmd_if_hook_set(
                caliptra_drivers::FipsTestHook::FW_LOAD_RUNTIME_DIGEST_FAILURE,
                caliptra_drivers::FipsTestHook::SHA384_DIGEST_FAILURE,
            )
        };

        let actual = self
            .env
            .sha384_digest(range.start, range.len() as u32)
            .map_err(|err| {
                self.env.set_fw_extended_error(err.into());
                CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_DIGEST_FAILURE
            })?;

        if cfi_launder(verify_info.digest) != actual {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_DIGEST_MISMATCH)?;
        } else {
            caliptra_cfi_lib::cfi_assert_eq_12_words(&verify_info.digest, &actual);
        }

        // Overflow/underflow is checked in verify_toc
        if !self.env.iccm_range().contains(&verify_info.load_addr)
            || !self
                .env
                .iccm_range()
                .contains(&(verify_info.load_addr + verify_info.size - 1))
        {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDR_INVALID)?;
        }
        if verify_info.load_addr % 4 != 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDR_UNALIGNED)?;
        }
        if !self.env.iccm_range().contains(&verify_info.entry_point) {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_ENTRY_POINT_INVALID)?;
        }
        if verify_info.entry_point % 4 != 0 {
            Err(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_ENTRY_POINT_UNALIGNED)?;
        }

        let info = ImageVerificationExeInfo {
            load_addr: verify_info.load_addr,
            entry_point: verify_info.entry_point,
            digest: verify_info.digest,
            size: verify_info.size,
        };

        Ok(info)
    }
}

#[cfg(all(test, target_family = "unix"))]
mod tests {
    use super::*;
    use caliptra_common::memory_layout::*;

    const DUMMY_DATA: [u32; 12] = [
        0xdeadbeef, 0xdeadbeef, 0xdeadbeef, 0xdeadbeef, 0xdeadbeef, 0xdeadbeef, 0xdeadbeef,
        0xdeadbeef, 0xdeadbeef, 0xdeadbeef, 0xdeadbeef, 0xdeadbeef,
    ];
    const VENDOR_ECC_PUBKEY: ImageEccPubKey = ImageEccPubKey {
        x: DUMMY_DATA,
        y: DUMMY_DATA,
    };
    const VENDOR_ECC_SIG: ImageEccSignature = ImageEccSignature {
        r: DUMMY_DATA,
        s: DUMMY_DATA,
    };
    fn vendor_lms_pubkey() -> ImageLmsPublicKey {
        ImageLmsPublicKey::default()
    }
    fn vendor_lms_sig() -> ImageLmsSignature {
        ImageLmsSignature::default()
    }
    const OWNER_ECC_PUBKEY: ImageEccPubKey = ImageEccPubKey {
        x: DUMMY_DATA,
        y: DUMMY_DATA,
    };
    const OWNER_ECC_SIG: ImageEccSignature = ImageEccSignature {
        r: DUMMY_DATA,
        s: DUMMY_DATA,
    };

    #[test]
    fn test_vendor_ecc_pk_idx_update_rst() {
        let test_env = TestEnv {
            verify_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let preamble = ImagePreamble {
            vendor_pub_key_info: ImageVendorPubKeyInfo {
                ecc_key_descriptor: ImageEccKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,
                    key_hash_count: 1,
                    reserved: 0,
                    key_hash: ImageEccKeyHashes::default(),
                },
                pqc_key_descriptor: ImagePqcKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,
                    key_type: FwVerificationPqcKeyType::LMS as u8,
                    key_hash_count: 1,
                    key_hash: ImagePqcKeyHashes::default(),
                },
            },
            ..Default::default()
        };

        let result = verifier.verify_vendor_ecc_pk_idx(&preamble, ResetReason::UpdateReset);
        assert!(result.is_ok());
    }

    #[test]
    fn test_vendor_ecc_pk_idx_mismatch_update_rst() {
        let test_env = TestEnv {
            verify_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);

        let preamble = ImagePreamble {
            vendor_pub_key_info: ImageVendorPubKeyInfo {
                ecc_key_descriptor: ImageEccKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,
                    key_hash_count: 4,
                    reserved: 0,
                    key_hash: ImageEccKeyHashes::default(),
                },
                pqc_key_descriptor: ImagePqcKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,
                    key_type: FwVerificationPqcKeyType::LMS as u8,
                    key_hash_count: 1,
                    key_hash: ImagePqcKeyHashes::default(),
                },
            },
            vendor_ecc_pub_key_idx: 2,
            ..Default::default()
        };

        let result = verifier.verify_vendor_ecc_pk_idx(&preamble, ResetReason::UpdateReset);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_VENDOR_ECC_PUB_KEY_IDX_MISMATCH)
        );
    }

    #[test]
    fn test_owner_pk_digest_update_rst() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            digest_384: DUMMY_DATA,
            ..Default::default()
        };

        let mut verifier = ImageVerifier::new(test_env);

        let preamble = ImagePreamble {
            vendor_pub_key_info: ImageVendorPubKeyInfo {
                ecc_key_descriptor: ImageEccKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_hash_count: 1,
                    reserved: 0,
                    key_hash: [DUMMY_DATA; 4],
                },
                pqc_key_descriptor: ImagePqcKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_type: FwVerificationPqcKeyType::LMS as u8,
                    key_hash_count: 1,
                    key_hash: [DUMMY_DATA; 32],
                },
            },
            ..Default::default()
        };

        let result = verifier.verify_preamble(
            &preamble,
            ResetReason::UpdateReset,
            FwVerificationPqcKeyType::LMS,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_fmc_update_rst() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            digest_384: DUMMY_DATA,
            fmc_digest: DUMMY_DATA,
            ..Default::default()
        };

        let mut verifier = ImageVerifier::new(test_env);

        let verify_info = ImageTocEntry {
            digest: DUMMY_DATA,
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: 100,
            ..Default::default()
        };

        let result = verifier.verify_fmc(&verify_info, ResetReason::UpdateReset);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_fmc_mismatch_update_rst() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            digest_384: DUMMY_DATA,
            ..Default::default()
        };

        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            digest: DUMMY_DATA,
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: 100,
            ..Default::default()
        };

        let result = verifier.verify_fmc(&verify_info, ResetReason::UpdateReset);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_FMC_DIGEST_MISMATCH)
        );
    }

    #[test]
    fn test_owner_verify_preamble_update_rst() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            digest_384: DUMMY_DATA,
            fmc_digest: DUMMY_DATA,
            ..Default::default()
        };

        let mut verifier = ImageVerifier::new(test_env);
        let preamble = ImagePreamble {
            vendor_pub_key_info: ImageVendorPubKeyInfo {
                ecc_key_descriptor: ImageEccKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_hash_count: 1,
                    reserved: 0,
                    key_hash: [DUMMY_DATA; 4],
                },
                pqc_key_descriptor: ImagePqcKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_type: FwVerificationPqcKeyType::LMS as u8,
                    key_hash_count: 1,
                    key_hash: [DUMMY_DATA; 32],
                },
            },
            ..Default::default()
        };

        let result = verifier.verify_preamble(
            &preamble,
            ResetReason::UpdateReset,
            FwVerificationPqcKeyType::LMS,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_manifest_marker() {
        let manifest = ImageManifest::default();
        let mut verifier = ImageVerifier::new(TestEnv::default());
        let result = verifier.verify(&manifest, manifest.size, ResetReason::ColdReset);
        assert!(result.is_err());
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_MANIFEST_MARKER_MISMATCH)
        );
    }

    #[test]
    fn test_manifest_size() {
        let manifest = ImageManifest {
            marker: MANIFEST_MARKER,
            size: 100,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(TestEnv::default());
        let result = verifier.verify(&manifest, manifest.size, ResetReason::ColdReset);
        assert!(result.is_err());
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_MANIFEST_SIZE_MISMATCH)
        );
    }

    #[test]
    fn test_preamble_vendor_pubkey_digest() {
        let preamble = ImagePreamble::default();
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let result = verifier.verify_preamble(
            &preamble,
            ResetReason::ColdReset,
            FwVerificationPqcKeyType::LMS,
        );
        assert!(result.is_err());
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_INVALID)
        );
    }

    #[test]
    fn test_preamble_owner_pubkey_digest() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            digest_384: DUMMY_DATA,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let preamble = ImagePreamble {
            vendor_pub_key_info: ImageVendorPubKeyInfo {
                ecc_key_descriptor: ImageEccKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_hash_count: 1,
                    reserved: 0,
                    key_hash: [DUMMY_DATA; 4],
                },
                pqc_key_descriptor: ImagePqcKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_type: FwVerificationPqcKeyType::LMS as u8,
                    key_hash_count: 1,
                    key_hash: [DUMMY_DATA; 32],
                },
            },
            ..Default::default()
        };

        let result = verifier.verify_preamble(
            &preamble,
            ResetReason::ColdReset,
            FwVerificationPqcKeyType::LMS,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_preamble_vendor_pubkey() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let preamble = ImagePreamble {
            vendor_pub_key_info: ImageVendorPubKeyInfo {
                ecc_key_descriptor: ImageEccKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_hash_count: 1,
                    reserved: 0,
                    key_hash: ImageEccKeyHashes::default(),
                },
                pqc_key_descriptor: ImagePqcKeyDescriptor {
                    version: KEY_DESCRIPTOR_VERSION,

                    key_type: FwVerificationPqcKeyType::LMS as u8,
                    key_hash_count: 1,
                    key_hash: ImagePqcKeyHashes::default(),
                },
            },
            ..Default::default()
        };
        let result = verifier.verify_preamble(
            &preamble,
            ResetReason::ColdReset,
            FwVerificationPqcKeyType::LMS,
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_MISMATCH)
        );
    }

    #[test]
    fn test_header_vendor_pubkey_invalid_arg() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let ecc_pubkey = ImageEccPubKey::default();
        let ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&ImageEccPubKey::default(), &ImageEccSignature::default()),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&ecc_pubkey, &ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INVALID_ARG)
        );
    }

    #[test]
    fn test_header_vendor_signature_invalid_arg() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_pubkey = ImageEccPubKey::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &ImageEccSignature::default()),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&owner_ecc_pubkey, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            // verified error
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID_ARG)
        );
    }

    #[test]
    fn test_header_vendor_signature_invalid() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_pubkey = ImageEccPubKey::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&owner_ecc_pubkey, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID)
        );
    }

    #[test]
    fn test_header_vendor_lms_signature_invalid() {
        let test_env = TestEnv {
            lifecycle: Lifecycle::Production,
            vendor_pub_key_digest: DUMMY_DATA,
            owner_pub_key_digest: DUMMY_DATA,
            verify_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_pubkey = ImageEccPubKey::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&owner_ecc_pubkey, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_LMS_SIGNATURE_INVALID)
        );
    }

    #[test]
    fn test_header_incorrect_pubkey_index() {
        let test_env = TestEnv {
            verify_result: true,
            verify_pqc_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_pubkey = ImageEccPubKey::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 1,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&owner_ecc_pubkey, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INDEX_MISMATCH)
        );
    }

    #[test]
    fn test_header_incorrect_lms_pubkey_index() {
        let test_env = TestEnv {
            verify_result: true,
            verify_pqc_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_pubkey = ImageEccPubKey::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 1,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&owner_ecc_pubkey, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_MISMATCH)
        );
    }

    #[test]
    fn test_header_owner_pubkey_invalid_arg() {
        let test_env = TestEnv {
            verify_result: true,
            verify_pqc_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_pubkey = ImageEccPubKey::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&owner_ecc_pubkey, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_PUB_KEY_INVALID_ARG)
        );
    }

    #[test]
    fn test_header_owner_signature_invalid_arg() {
        let test_env = TestEnv {
            verify_result: true,
            verify_pqc_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader::default();
        let owner_ecc_sig = ImageEccSignature::default();
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&OWNER_ECC_PUBKEY, &owner_ecc_sig),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let result = verifier.verify_header(&header, &header_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_SIGNATURE_INVALID_ARG)
        );
    }

    #[test]
    fn test_header_success() {
        let test_env = TestEnv {
            verify_result: true,
            verify_pqc_result: true,
            ..Default::default()
        };
        let mut verifier = ImageVerifier::new(test_env);
        let header = ImageHeader {
            toc_len: 100,
            toc_digest: DUMMY_DATA,
            ..Default::default()
        };
        let owner_lms_pubkey = ImageLmsPublicKey::default();
        let owner_lms_sig = ImageLmsSignature::default();
        let binding_vendor_lms_pubkey = vendor_lms_pubkey();
        let binding_vendor_lms_sig = vendor_lms_sig();
        let header_info: HeaderInfo = HeaderInfo {
            vendor_ecc_pub_key_idx: 0,
            vendor_pqc_pub_key_idx: 0,
            vendor_ecc_info: (&VENDOR_ECC_PUBKEY, &VENDOR_ECC_SIG),
            vendor_pqc_info: PqcKeyInfo::Lms(&binding_vendor_lms_pubkey, &binding_vendor_lms_sig),
            owner_ecc_info: (&OWNER_ECC_PUBKEY, &OWNER_ECC_SIG),
            owner_pqc_info: PqcKeyInfo::Lms(&owner_lms_pubkey, &owner_lms_sig),
            owner_pub_keys_digest: ImageDigest384::default(),
            owner_pub_keys_digest_in_fuses: false,
            vendor_ecc_pub_key_revocation: Default::default(),
            vendor_pqc_pub_key_revocation: Default::default(),
        };
        let toc_info = verifier.verify_header(&header, &header_info).unwrap();
        assert_eq!(toc_info.len, 100);
        assert_eq!(toc_info.digest, &DUMMY_DATA);
    }

    #[test]
    fn test_toc_incorrect_length() {
        let manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT / 2,
            digest: &ImageDigest384::default(),
        };
        let result = verifier.verify_toc(&manifest, &toc_info, manifest.size);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_TOC_ENTRY_COUNT_INVALID)
        );
    }

    #[test]
    fn test_toc_digest_mismatch() {
        let manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT,
            digest: &DUMMY_DATA,
        };
        let result = verifier.verify_toc(&manifest, &toc_info, manifest.size);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_TOC_DIGEST_MISMATCH)
        );
    }

    #[test]
    fn test_toc_fmc_rt_overlap() {
        let mut manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT,
            digest: &ImageDigest384::default(),
        };

        // Case 0:
        // [-FMC--]
        // [--RT--]
        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 0;
        manifest.runtime.size = 100;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );

        // Case 1:
        // [-FMC--]
        //        [--RT--]
        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 99;
        manifest.runtime.size = 200;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );

        // Case 2:
        // [-FMC--]
        //   [-RT-]
        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 5;
        manifest.runtime.size = 100;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );

        // Case 3:
        //   [-FMC-]
        // [---RT--]
        manifest.fmc.offset = 5;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 0;
        manifest.runtime.size = 100;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );

        // Case 4:
        //        [-FMC--]
        // [--RT--]
        manifest.runtime.offset = 0;
        manifest.runtime.size = 100;
        manifest.fmc.offset = 99;
        manifest.fmc.size = 200;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );

        // Case 5:
        //  [---FMC---]
        //    [-RT-]
        manifest.fmc.offset = 100;
        manifest.fmc.size = 500;
        manifest.runtime.offset = 150;
        manifest.runtime.size = 200;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );

        // Case 6:
        //  [----RT----]
        //    [-FMC-]
        manifest.runtime.offset = 0;
        manifest.runtime.size = 200;
        manifest.fmc.offset = 20;
        manifest.fmc.size = 30;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP)
        );
    }

    #[test]
    fn test_size_failure() {
        let mut manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT,
            digest: &ImageDigest384::default(),
        };

        // FMC size == 0
        manifest.fmc.offset = 0;
        manifest.fmc.size = 0;
        manifest.runtime.offset = 100;
        manifest.runtime.size = 200;
        let result = verifier.verify_toc(&manifest, &toc_info, 500);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_SIZE_ZERO)
        );

        // RT size == 0
        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 100;
        manifest.runtime.size = 0;
        let result = verifier.verify_toc(&manifest, &toc_info, 500);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_SIZE_ZERO)
        );

        // [-FMC--]
        // [--RT--]
        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 100;
        manifest.runtime.size = 200;
        let result = verifier.verify_toc(&manifest, &toc_info, 100);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_IMAGE_LEN_MORE_THAN_BUNDLE_SIZE)
        );
    }

    #[test]
    fn test_size_success() {
        let mut manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT,
            digest: &ImageDigest384::default(),
        };

        // [-FMC--]
        // [--RT--]
        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 100;
        manifest.runtime.size = 200;
        manifest.fmc.load_addr = 0x1000;
        manifest.runtime.load_addr = 0x2000;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_toc_fmc_rt_order() {
        let mut manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT,
            digest: &ImageDigest384::default(),
        };

        // [-FMC--]
        // [--RT--]
        manifest.runtime.offset = 0;
        manifest.runtime.size = 100;
        manifest.fmc.offset = 100;
        manifest.fmc.size = 200;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_INCORRECT_ORDER)
        );
    }

    #[test]
    fn test_fmc_rt_load_address_range_overlap() {
        let mut manifest = ImageManifest::default();
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let toc_info = TocInfo {
            len: MAX_TOC_ENTRY_COUNT,
            digest: &ImageDigest384::default(),
        };

        manifest.fmc.offset = 0;
        manifest.fmc.size = 100;
        manifest.runtime.offset = 100;
        manifest.runtime.size = 200;

        // Case 1:
        // [-FMC--]
        //      [--RT--]
        manifest.fmc.load_addr = 0;
        manifest.fmc.size = 100;
        manifest.runtime.load_addr = 50;
        manifest.runtime.size = 100;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_LOAD_ADDR_OVERLAP)
        );

        // Case 2:
        //      [-FMC--]
        //  [--RT--]
        manifest.fmc.load_addr = 50;
        manifest.fmc.size = 100;
        manifest.runtime.load_addr = 0;
        manifest.runtime.size = 100;
        let result = verifier.verify_toc(
            &manifest,
            &toc_info,
            manifest.size + manifest.fmc.image_size() + manifest.runtime.image_size(),
        );

        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_LOAD_ADDR_OVERLAP)
        );
    }

    #[test]
    fn test_fmc_contained_in_iccm() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: ICCM_SIZE + 1,
            ..Default::default()
        };

        let result = verifier.verify_fmc(&verify_info, ResetReason::ColdReset);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDR_INVALID)
        );

        let verify_info = ImageTocEntry {
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: ICCM_SIZE,
            ..Default::default()
        };

        let result = verifier.verify_fmc(&verify_info, ResetReason::ColdReset);
        assert_eq!(result.err(), None);
    }

    #[test]
    fn test_fmc_digest_mismatch() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            digest: DUMMY_DATA,
            ..Default::default()
        };
        let result = verifier.verify_fmc(&verify_info, ResetReason::ColdReset);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_FMC_DIGEST_MISMATCH)
        );
    }

    #[test]
    fn test_fmc_success() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: 100,
            ..Default::default()
        };

        let result = verifier.verify_fmc(&verify_info, ResetReason::ColdReset);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.load_addr, ICCM_ORG);
        assert_eq!(info.entry_point, ICCM_ORG);
        assert_eq!(info.size, 100);
    }

    #[test]
    fn test_rt_digest_mismatch() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            digest: DUMMY_DATA,
            ..Default::default()
        };
        let result = verifier.verify_runtime(&verify_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_DIGEST_MISMATCH)
        );
    }

    #[test]
    fn test_rt_contained_in_iccm() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: ICCM_SIZE + 1,
            ..Default::default()
        };

        let result = verifier.verify_runtime(&verify_info);
        assert_eq!(
            result.err(),
            Some(CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDR_INVALID)
        );

        let verify_info = ImageTocEntry {
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: ICCM_SIZE,
            ..Default::default()
        };

        let result = verifier.verify_runtime(&verify_info);
        assert_eq!(result.err(), None);
    }

    #[test]
    fn test_rt_success() {
        let test_env = TestEnv::default();
        let mut verifier = ImageVerifier::new(test_env);
        let verify_info = ImageTocEntry {
            load_addr: ICCM_ORG,
            entry_point: ICCM_ORG,
            size: 100,
            ..Default::default()
        };
        let result = verifier.verify_runtime(&verify_info);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.load_addr, ICCM_ORG);
        assert_eq!(info.entry_point, ICCM_ORG);
        assert_eq!(info.size, 100);
    }

    struct TestEnv {
        digest_384: ImageDigest384,
        digest_512: ImageDigest512,
        fmc_digest: ImageDigest384,
        verify_result: bool,
        verify_pqc_result: bool,
        vendor_pub_key_digest: ImageDigest384,
        vendor_ecc_pub_key_revocation: VendorEccPubKeyRevocation,
        vendor_pqc_pub_key_revocation: u32,
        owner_pub_key_digest: ImageDigest384,
        lifecycle: Lifecycle,
        pqc_key_type: FwVerificationPqcKeyType,
    }

    impl Default for TestEnv {
        fn default() -> Self {
            TestEnv {
                digest_384: ImageDigest384::default(),
                digest_512: ImageDigest512::default(),
                fmc_digest: ImageDigest384::default(),
                verify_result: false,
                verify_pqc_result: false,
                vendor_pub_key_digest: ImageDigest384::default(),
                vendor_ecc_pub_key_revocation: VendorEccPubKeyRevocation::default(),
                vendor_pqc_pub_key_revocation: 0,
                owner_pub_key_digest: ImageDigest384::default(),
                lifecycle: Lifecycle::Unprovisioned,
                pqc_key_type: FwVerificationPqcKeyType::MLDSA,
            }
        }
    }

    impl ImageVerificationEnv for TestEnv {
        fn sha384_digest(&mut self, _offset: u32, _len: u32) -> CaliptraResult<ImageDigest384> {
            Ok(self.digest_384)
        }

        fn sha512_digest(&mut self, _offset: u32, _len: u32) -> CaliptraResult<ImageDigest512> {
            Ok(self.digest_512)
        }

        fn ecc384_verify(
            &mut self,
            _digest: &ImageDigest384,
            _pub_key: &ImageEccPubKey,
            sig: &ImageEccSignature,
        ) -> CaliptraResult<Array4xN<12, 48>> {
            if self.verify_result {
                Ok(Array4x12::from(sig.r))
            } else {
                Ok(Array4x12::from(&[0xFF; 48]))
            }
        }

        fn lms_verify(
            &mut self,
            _digest: &ImageDigest384,
            pub_key: &ImageLmsPublicKey,
            _sig: &ImageLmsSignature,
        ) -> CaliptraResult<HashValue<SHA192_DIGEST_WORD_SIZE>> {
            if self.verify_pqc_result {
                Ok(HashValue::from(pub_key.digest))
            } else {
                Ok(HashValue::from(&[0xDEADBEEF; 6]))
            }
        }

        fn mldsa87_verify(
            &mut self,
            _digest: &ImageDigest512,
            _pub_key: &ImageMldsaPubKey,
            _sig: &ImageMldsaSignature,
        ) -> CaliptraResult<Mldsa87Result> {
            if self.verify_pqc_result {
                Ok(Mldsa87Result::Success)
            } else {
                Ok(Mldsa87Result::SigVerifyFailed)
            }
        }

        fn vendor_pub_key_info_digest_fuses(&self) -> ImageDigest384 {
            self.vendor_pub_key_digest
        }

        fn vendor_ecc_pub_key_revocation(&self) -> VendorEccPubKeyRevocation {
            self.vendor_ecc_pub_key_revocation
        }

        fn vendor_lms_pub_key_revocation(&self) -> u32 {
            self.vendor_pqc_pub_key_revocation
        }

        fn vendor_mldsa_pub_key_revocation(&self) -> u32 {
            self.vendor_pqc_pub_key_revocation
        }

        fn owner_pub_key_digest_fuses(&self) -> ImageDigest384 {
            self.owner_pub_key_digest
        }

        fn anti_rollback_disable(&self) -> bool {
            false
        }

        fn dev_lifecycle(&self) -> Lifecycle {
            self.lifecycle
        }

        fn vendor_ecc_pub_key_idx_dv(&self) -> u32 {
            0
        }

        fn vendor_pqc_pub_key_idx_dv(&self) -> u32 {
            0
        }

        fn owner_pub_key_digest_dv(&self) -> ImageDigest384 {
            self.owner_pub_key_digest
        }

        fn get_fmc_digest_dv(&self) -> ImageDigest384 {
            self.fmc_digest
        }

        fn fw_fuse_svn(&self) -> u32 {
            0
        }

        fn iccm_range(&self) -> Range<u32> {
            Range {
                start: ICCM_ORG,
                end: ICCM_ORG + ICCM_SIZE,
            }
        }

        fn pqc_key_type_fuse(&self) -> CaliptraResult<FwVerificationPqcKeyType> {
            Ok(self.pqc_key_type)
        }

        fn set_fw_extended_error(&mut self, _err: u32) {}
    }
}
