/*++

Licensed under the Apache-2.0 license.

File Name:

    fw_processor.rs

Abstract:

    File contains the code to download and validate the firmware.

--*/

#[cfg(feature = "fake-rom")]
use crate::flow::fake::FakeRomImageVerificationEnv;
use crate::fuse::log_fuse_data;
use crate::key_ladder;
use crate::pcr;
use crate::rom_env::RomEnv;
use crate::run_fips_tests;
#[cfg(not(feature = "no-cfi"))]
use caliptra_cfi_derive::cfi_impl_fn;
use caliptra_cfi_lib::CfiCounter;
use caliptra_common::capabilities::Capabilities;
use caliptra_common::fips::FipsVersionCmd;
use caliptra_common::mailbox_api::{
    CapabilitiesResp, CommandId, GetIdevCsrResp, MailboxReqHeader, MailboxRespHeader, Response,
    StashMeasurementReq, StashMeasurementResp,
};
use caliptra_common::{
    pcr::PCR_ID_STASH_MEASUREMENT, verifier::FirmwareImageVerificationEnv, FuseLogEntryId,
    PcrLogEntry, PcrLogEntryId, RomBootStatus::*,
};
use caliptra_drivers::{pcr_log::MeasurementLogEntry, *};
use caliptra_image_types::{FwVerificationPqcKeyType, ImageManifest, IMAGE_BYTE_SIZE};
use caliptra_image_verify::MAX_FIRMWARE_SVN;
use caliptra_image_verify::{ImageVerificationInfo, ImageVerificationLogInfo, ImageVerifier};
use caliptra_kat::KatsEnv;
use caliptra_x509::{NotAfter, NotBefore};
use core::mem::{size_of, ManuallyDrop};
use zerocopy::{FromBytes, IntoBytes};
use zeroize::Zeroize;

const RESERVED_PAUSER: u32 = 0xFFFFFFFF;

#[derive(Debug, Default, Zeroize)]
pub struct FwProcInfo {
    pub fmc_cert_valid_not_before: NotBefore,

    pub fmc_cert_valid_not_after: NotAfter,

    pub effective_fuse_svn: u32,

    pub owner_pub_keys_digest_in_fuses: bool,

    pub pqc_key_type: u8,
}

pub struct FirmwareProcessor {}

impl FirmwareProcessor {
    pub fn process(env: &mut RomEnv) -> CaliptraResult<FwProcInfo> {
        let mut kats_env = caliptra_kat::KatsEnv {
            // SHA1 Engine
            sha1: &mut env.sha1,

            // sha256
            sha256: &mut env.sha256,

            // SHA2-512/384 Engine
            sha2_512_384: &mut env.sha2_512_384,

            // SHA2-512/384 Accelerator
            sha2_512_384_acc: &mut env.sha2_512_384_acc,

            // Hmac-512/384 Engine
            hmac: &mut env.hmac,

            /// Cryptographically Secure Random Number Generator
            trng: &mut env.trng,

            // LMS Engine
            lms: &mut env.lms,

            // Mldsa87 Engine
            mldsa87: &mut env.mldsa87,

            /// Ecc384 Engine
            ecc384: &mut env.ecc384,

            /// SHA Acc lock state
            sha_acc_lock_state: ShaAccLockState::NotAcquired,
        };
        // Process mailbox commands.
        let (mut txn, image_size_bytes) = Self::process_mailbox_commands(
            &mut env.soc_ifc,
            &mut env.mbox,
            &mut env.pcr_bank,
            &mut env.dma,
            &mut kats_env,
            env.persistent_data.get_mut(),
        )?;

        #[cfg(feature = "fips-test-hooks")]
        unsafe {
            caliptra_drivers::FipsTestHook::halt_if_hook_set(
                caliptra_drivers::FipsTestHook::HALT_FW_LOAD,
            )
        };

        // Load the manifest into DCCM.
        let manifest = Self::load_manifest(
            &mut env.persistent_data,
            &mut txn,
            env.soc_ifc.active_mode(),
        );
        let manifest = okref(&manifest)?;

        let mut venv = FirmwareImageVerificationEnv {
            sha256: &mut env.sha256,
            sha2_512_384: &mut env.sha2_512_384,
            soc_ifc: &mut env.soc_ifc,
            ecc384: &mut env.ecc384,
            mldsa87: &mut env.mldsa87,
            data_vault: &env.persistent_data.get().data_vault,
            pcr_bank: &mut env.pcr_bank,
            image: txn.raw_mailbox_contents(),
        };

        // Verify the image
        let info = Self::verify_image(&mut venv, manifest, image_size_bytes);
        let info = okref(&info)?;

        Self::update_fuse_log(&mut env.persistent_data.get_mut().fuse_log, &info.log_info)?;

        // Populate data vault
        Self::populate_data_vault(info, &mut env.persistent_data);

        // Extend PCR0 and PCR1
        pcr::extend_pcrs(
            env.persistent_data.get_mut(),
            &env.soc_ifc,
            &mut env.pcr_bank,
            &mut env.sha2_512_384,
            info,
        )?;
        report_boot_status(FwProcessorExtendPcrComplete.into());

        // Load the image
        Self::load_image(manifest, &mut txn, env.soc_ifc.active_mode())?;

        // Complete the mailbox transaction indicating success.
        txn.complete(true)?;
        report_boot_status(FwProcessorFirmwareDownloadTxComplete.into());

        // Update FW version registers
        // Truncate FMC version to 16 bits (no error for 31:16 != 0)
        env.soc_ifc.set_fmc_fw_rev_id(manifest.fmc.version as u16);
        env.soc_ifc.set_rt_fw_rev_id(manifest.runtime.version);

        // Get the certificate validity info
        let (nb, nf) = Self::get_cert_validity_info(manifest);

        Self::populate_fw_key_ladder(env)?;

        report_boot_status(FwProcessorComplete.into());
        Ok(FwProcInfo {
            fmc_cert_valid_not_before: nb,
            fmc_cert_valid_not_after: nf,
            effective_fuse_svn: info.effective_fuse_svn,
            owner_pub_keys_digest_in_fuses: info.owner_pub_keys_digest_in_fuses,
            pqc_key_type: info.pqc_key_type as u8,
        })
    }

    /// Process mailbox commands
    ///
    /// # Arguments
    ///
    /// * `soc_ifc` - SOC Interface
    /// * `mbox` - Mailbox
    /// * `pcr_bank` - PCR Bank
    /// * `dma` - DMA engine
    /// * `env` - KAT Environment
    /// * `persistent_data` - Persistent data
    ///
    /// # Returns
    /// * `MailboxRecvTxn` - Mailbox Receive Transaction
    ///
    /// Mailbox transaction handle (returned only for the FIRMWARE_LOAD command).
    /// This transaction is ManuallyDrop because we don't want the transaction
    /// to be completed with failure until after handle_fatal_error is called.
    /// This prevents a race condition where the SoC reads FW_ERROR_NON_FATAL
    /// immediately after the mailbox transaction fails,
    ///  but before caliptra has set the FW_ERROR_NON_FATAL register.
    fn process_mailbox_commands<'a>(
        soc_ifc: &mut SocIfc,
        mbox: &'a mut Mailbox,
        pcr_bank: &mut PcrBank,
        dma: &mut Dma,
        env: &mut KatsEnv,
        persistent_data: &mut PersistentData,
    ) -> CaliptraResult<(ManuallyDrop<MailboxRecvTxn<'a>>, u32)> {
        let mut self_test_in_progress = false;
        let active_mode = soc_ifc.active_mode();

        cprintln!("[fwproc] Wait for Commands...");
        loop {
            // Random delay for CFI glitch protection.
            CfiCounter::delay();

            if let Some(txn) = mbox.peek_recv() {
                report_fw_error_non_fatal(0);

                // Drop all commands for invalid PAUSER
                if txn.id() == RESERVED_PAUSER {
                    return Err(CaliptraError::FW_PROC_MAILBOX_RESERVED_PAUSER);
                }

                cprintln!("[fwproc] Recv command 0x{:08x}", txn.cmd());

                // Handle FW load as a separate case due to the re-borrow explained below
                if txn.cmd() == CommandId::FIRMWARE_LOAD.into() {
                    if active_mode {
                        Err(CaliptraError::FW_PROC_MAILBOX_FW_LOAD_CMD_IN_ACTIVE_MODE)?;
                    }

                    // Re-borrow mailbox to work around https://github.com/rust-lang/rust/issues/54663
                    let txn = mbox
                        .peek_recv()
                        .ok_or(CaliptraError::FW_PROC_MAILBOX_STATE_INCONSISTENT)?;

                    // This is a download-firmware command; don't drop this, as the
                    // transaction will be completed by either handle_fatal_error() (on
                    // failure) or by a manual complete call upon success.
                    let txn = ManuallyDrop::new(txn.start_txn());
                    let image_size_bytes = txn.dlen();
                    if image_size_bytes == 0 || image_size_bytes > IMAGE_BYTE_SIZE as u32 {
                        cprintln!("Invalid Image of size {} bytes", image_size_bytes);
                        return Err(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE);
                    }

                    cprintln!("[fwproc] Received Image of size {} bytes", image_size_bytes);
                    report_boot_status(FwProcessorDownloadImageComplete.into());
                    return Ok((txn, image_size_bytes));
                }

                // NOTE: We use ManuallyDrop here because any error here becomes a fatal error
                //       See note above about race condition
                let mut txn = ManuallyDrop::new(txn.start_txn());
                match CommandId::from(txn.cmd()) {
                    CommandId::VERSION => {
                        let mut request = MailboxReqHeader::default();
                        Self::copy_req_verify_chksum(&mut txn, request.as_mut_bytes())?;

                        let mut resp = FipsVersionCmd::execute(soc_ifc)?;
                        resp.populate_chksum();
                        txn.send_response(resp.as_bytes())?;
                    }
                    CommandId::SELF_TEST_START => {
                        let mut request = MailboxReqHeader::default();
                        Self::copy_req_verify_chksum(&mut txn, request.as_mut_bytes())?;

                        if self_test_in_progress {
                            // TODO: set non-fatal error register?
                            txn.complete(false)?;
                        } else {
                            run_fips_tests(env)?;
                            let mut resp = MailboxRespHeader::default();
                            resp.populate_chksum();
                            txn.send_response(resp.as_bytes())?;
                            self_test_in_progress = true;
                        }
                    }
                    CommandId::SELF_TEST_GET_RESULTS => {
                        let mut request = MailboxReqHeader::default();
                        Self::copy_req_verify_chksum(&mut txn, request.as_mut_bytes())?;

                        if !self_test_in_progress {
                            // TODO: set non-fatal error register?
                            txn.complete(false)?;
                        } else {
                            let mut resp = MailboxRespHeader::default();
                            resp.populate_chksum();
                            txn.send_response(resp.as_bytes())?;
                            self_test_in_progress = false;
                        }
                    }
                    CommandId::SHUTDOWN => {
                        let mut request = MailboxReqHeader::default();
                        Self::copy_req_verify_chksum(&mut txn, request.as_mut_bytes())?;

                        let mut resp = MailboxRespHeader::default();
                        resp.populate_chksum();
                        txn.send_response(resp.as_bytes())?;

                        // Causing a ROM Fatal Error will zeroize the module
                        return Err(CaliptraError::RUNTIME_SHUTDOWN);
                    }
                    CommandId::CAPABILITIES => {
                        let mut request = MailboxReqHeader::default();
                        Self::copy_req_verify_chksum(&mut txn, request.as_mut_bytes())?;

                        let mut capabilities = Capabilities::default();
                        capabilities |= Capabilities::ROM_BASE;

                        let mut resp = CapabilitiesResp {
                            hdr: MailboxRespHeader::default(),
                            capabilities: capabilities.to_bytes(),
                        };
                        resp.populate_chksum();
                        txn.send_response(resp.as_bytes())?;
                        continue;
                    }
                    CommandId::STASH_MEASUREMENT => {
                        if persistent_data.fht.meas_log_index == MEASUREMENT_MAX_COUNT as u32 {
                            cprintln!("[fwproc] Max # of measurements received.");
                            txn.complete(false)?;

                            // Raise a fatal error on hitting the max. limit.
                            // This ensures that any SOC ROM/FW couldn't send a stash measurement
                            // that wasn't properly stored within Caliptra.
                            return Err(CaliptraError::FW_PROC_MAILBOX_STASH_MEASUREMENT_MAX_LIMIT);
                        }

                        Self::stash_measurement(
                            pcr_bank,
                            env.sha2_512_384,
                            persistent_data,
                            &mut txn,
                        )?;

                        // Generate and send response (with FIPS approved status)
                        let mut resp = StashMeasurementResp {
                            hdr: MailboxRespHeader::default(),
                            dpe_result: 0, // DPE_STATUS_SUCCESS
                        };
                        resp.populate_chksum();
                        txn.send_response(resp.as_bytes())?;
                    }
                    CommandId::GET_IDEV_ECC_CSR => {
                        let mut request = MailboxReqHeader::default();
                        Self::copy_req_verify_chksum(&mut txn, request.as_mut_bytes())?;

                        let csr_persistent_mem = &persistent_data.idevid_csr_envelop.ecc_csr;
                        let mut resp = GetIdevCsrResp::default();

                        if csr_persistent_mem.is_unprovisioned() {
                            // CSR was never written to DCCM. This means the gen_idev_id_csr
                            // manufacturing flag was not set before booting into ROM.
                            return Err(
                                CaliptraError::FW_PROC_MAILBOX_GET_IDEV_CSR_UNPROVISIONED_CSR,
                            );
                        }

                        let csr = csr_persistent_mem
                            .get()
                            .ok_or(CaliptraError::ROM_IDEVID_INVALID_CSR)?;

                        resp.data_size = csr_persistent_mem.get_csr_len();
                        resp.data[..resp.data_size as usize].copy_from_slice(csr);

                        resp.populate_chksum();
                        txn.send_response(resp.as_bytes())?;
                    }
                    CommandId::RI_DOWNLOAD_FIRMWARE => {
                        if !active_mode {
                            cprintln!(
                                "[fwproc] RI_DOWNLOAD_FIRMWARE cmd not supported in passive mode"
                            );
                            txn.complete(false)?;
                            Err(CaliptraError::FW_PROC_MAILBOX_INVALID_COMMAND)?;
                        }

                        // Download the firmware image from the recovery interface.
                        let image_size_bytes =
                            Self::retrieve_image_from_recovery_interface(dma, soc_ifc)?;
                        let txn = ManuallyDrop::new(mbox.fake_recv_txn());
                        cprintln!(
                            "[fwproc] Received Image from Recovery Interface of size {} bytes",
                            image_size_bytes
                        );
                        report_boot_status(FwProcessorDownloadImageComplete.into());
                        return Ok((txn, image_size_bytes));
                    }
                    _ => {
                        cprintln!("[fwproc] Invalid command received");
                        // Don't complete the transaction here; let the fatal
                        // error handler do it to prevent a race condition
                        // setting the error code.
                        return Err(CaliptraError::FW_PROC_MAILBOX_INVALID_COMMAND);
                    }
                }
            }
        }
    }

    /// Load the manifest
    ///
    /// # Returns
    ///
    /// * `Manifest` - Caliptra Image Bundle Manifest
    #[cfg_attr(not(feature = "no-cfi"), cfi_impl_fn)]
    fn load_manifest(
        persistent_data: &mut PersistentDataAccessor,
        txn: &mut MailboxRecvTxn,
        active_mode: bool,
    ) -> CaliptraResult<ImageManifest> {
        let manifest = &mut persistent_data.get_mut().manifest1;
        if active_mode {
            let mbox_sram = txn.raw_mailbox_contents();
            let manifest_buf = manifest.as_mut_bytes();
            if mbox_sram.len() < manifest_buf.len() {
                Err(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE)?;
            }
            manifest_buf.copy_from_slice(&mbox_sram[..manifest_buf.len()]);
        } else {
            txn.copy_request(manifest.as_mut_bytes())?;
        }
        report_boot_status(FwProcessorManifestLoadComplete.into());
        Ok(*manifest)
    }

    /// Verify the image
    ///
    /// # Arguments
    ///
    /// * `env` - ROM Environment
    #[cfg_attr(not(feature = "no-cfi"), cfi_impl_fn)]
    fn verify_image(
        venv: &mut FirmwareImageVerificationEnv,
        manifest: &ImageManifest,
        img_bundle_sz: u32,
    ) -> CaliptraResult<ImageVerificationInfo> {
        #[cfg(feature = "fake-rom")]
        let venv = &mut FakeRomImageVerificationEnv {
            sha256: venv.sha256,
            sha2_512_384: venv.sha2_512_384,
            soc_ifc: venv.soc_ifc,
            data_vault: venv.data_vault,
            ecc384: venv.ecc384,
            mldsa87: venv.mldsa87,
            image: venv.image,
        };

        // Random delay for CFI glitch protection.
        CfiCounter::delay();
        CfiCounter::delay();
        CfiCounter::delay();
        CfiCounter::delay();

        let mut verifier = ImageVerifier::new(venv);
        let info = verifier.verify(manifest, img_bundle_sz, ResetReason::ColdReset)?;

        cprintln!(
            "[fwproc] Img verified w/ Vendor ECC Key Idx {}, PQC Key Type: {}, PQC Key Idx {}, with SVN {} and effective fuse SVN {}",
            info.vendor_ecc_pub_key_idx,
            if FwVerificationPqcKeyType::from_u8(manifest.pqc_key_type) == Some(FwVerificationPqcKeyType::MLDSA)  { "MLDSA" } else { "LMS" },
            info.vendor_pqc_pub_key_idx,
            info.fw_svn,
            info.effective_fuse_svn,
        );
        report_boot_status(FwProcessorImageVerificationComplete.into());
        Ok(info)
    }

    /// Update the fuse log
    ///
    /// # Arguments
    /// * `log_info` - Image Verification Log Info
    ///
    /// # Returns
    /// * CaliptraResult
    #[cfg_attr(not(feature = "no-cfi"), cfi_impl_fn)]
    fn update_fuse_log(
        log: &mut FuseLogArray,
        log_info: &ImageVerificationLogInfo,
    ) -> CaliptraResult<()> {
        // Log VendorPubKeyIndex
        log_fuse_data(
            log,
            FuseLogEntryId::VendorEccPubKeyIndex,
            log_info.vendor_ecc_pub_key_idx.as_bytes(),
        )?;

        // Log VendorPubKeyRevocation
        log_fuse_data(
            log,
            FuseLogEntryId::VendorEccPubKeyRevocation,
            log_info
                .fuse_vendor_ecc_pub_key_revocation
                .bits()
                .as_bytes(),
        )?;

        // Log cold-boot FW SVN
        log_fuse_data(
            log,
            FuseLogEntryId::ColdBootFwSvn,
            log_info.fw_log_info.manifest_svn.as_bytes(),
        )?;

        // Log ManifestReserved0
        log_fuse_data(
            log,
            FuseLogEntryId::ManifestReserved0,
            log_info.fw_log_info.reserved.as_bytes(),
        )?;

        // Log DeprecatedFuseFmcSvn (which is now the same as FuseFwSvn)
        #[allow(deprecated)]
        log_fuse_data(
            log,
            FuseLogEntryId::_DeprecatedFuseFmcSvn,
            log_info.fw_log_info.fuse_svn.as_bytes(),
        )?;

        // Log ManifestFwSvn
        log_fuse_data(
            log,
            FuseLogEntryId::ManifestFwSvn,
            log_info.fw_log_info.manifest_svn.as_bytes(),
        )?;

        // Log ManifestReserved1
        log_fuse_data(
            log,
            FuseLogEntryId::ManifestReserved1,
            log_info.fw_log_info.reserved.as_bytes(),
        )?;

        // Log FuseFwSvn
        log_fuse_data(
            log,
            FuseLogEntryId::FuseFwSvn,
            log_info.fw_log_info.fuse_svn.as_bytes(),
        )?;

        // Log VendorPqcPubKeyIndex
        log_fuse_data(
            log,
            FuseLogEntryId::VendorPqcPubKeyIndex,
            log_info.vendor_pqc_pub_key_idx.as_bytes(),
        )?;

        // Log VendorPqcPubKeyRevocation
        log_fuse_data(
            log,
            FuseLogEntryId::VendorPqcPubKeyRevocation,
            log_info.fuse_vendor_pqc_pub_key_revocation.as_bytes(),
        )?;

        Ok(())
    }

    /// Load the image to ICCM & DCCM
    ///
    /// # Arguments
    ///
    /// * `manifest` - Manifest
    /// * `txn`      - Mailbox Receive Transaction
    /// * `active_mode` - Indicates if ROM is running in the Active mode
    // Inlined to reduce ROM size
    #[inline(always)]
    #[cfg_attr(not(feature = "no-cfi"), cfi_impl_fn)]
    fn load_image(
        manifest: &ImageManifest,
        txn: &mut MailboxRecvTxn,
        active_mode: bool,
    ) -> CaliptraResult<()> {
        cprintln!(
            "[fwproc] Load FMC at address 0x{:08x} len {}",
            manifest.fmc.load_addr,
            manifest.fmc.size
        );

        if active_mode {
            let mbox_sram = txn.raw_mailbox_contents();
            let fmc_dest = unsafe {
                let addr = (manifest.fmc.load_addr) as *mut u8;
                core::slice::from_raw_parts_mut(addr, manifest.fmc.size as usize)
            };
            let start = size_of::<ImageManifest>();
            let end = start + fmc_dest.len();
            if start > end || mbox_sram.len() < end {
                Err(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE)?;
            }
            fmc_dest.copy_from_slice(&mbox_sram[start..end]);
        } else {
            let fmc_dest = unsafe {
                let addr = (manifest.fmc.load_addr) as *mut u32;
                core::slice::from_raw_parts_mut(addr, manifest.fmc.size as usize / 4)
            };

            txn.copy_request(fmc_dest.as_mut_bytes())?;
        }

        cprintln!(
            "[fwproc] Load Runtime at address 0x{:08x} len {}",
            manifest.runtime.load_addr,
            manifest.runtime.size
        );

        if active_mode {
            let mbox_sram = txn.raw_mailbox_contents();
            let runtime_dest = unsafe {
                let addr = (manifest.runtime.load_addr) as *mut u8;
                core::slice::from_raw_parts_mut(addr, manifest.runtime.size as usize)
            };
            let start = size_of::<ImageManifest>() + manifest.fmc.size as usize;
            let end = start + runtime_dest.len();
            if start > end || mbox_sram.len() < end {
                Err(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE)?;
            }
            runtime_dest.copy_from_slice(&mbox_sram[start..end]);
        } else {
            let runtime_dest = unsafe {
                let addr = (manifest.runtime.load_addr) as *mut u32;
                core::slice::from_raw_parts_mut(addr, manifest.runtime.size as usize / 4)
            };

            txn.copy_request(runtime_dest.as_mut_bytes())?;
        }

        report_boot_status(FwProcessorLoadImageComplete.into());
        Ok(())
    }

    /// Populate data vault
    ///
    /// # Arguments
    ///
    /// * `info` - Image Verification Info
    /// * `persistent_data` - Persistent data accessor
    ///
    #[cfg_attr(not(feature = "no-cfi"), cfi_impl_fn)]
    fn populate_data_vault(
        info: &ImageVerificationInfo,
        persistent_data: &mut PersistentDataAccessor,
    ) {
        let manifest_address = &persistent_data.get().manifest1 as *const _ as u32;
        let data_vault = &mut persistent_data.get_mut().data_vault;
        data_vault.set_fmc_tci(&info.fmc.digest.into());
        data_vault.set_cold_boot_fw_svn(info.fw_svn);
        data_vault.set_fmc_entry_point(info.fmc.entry_point);
        data_vault.set_owner_pk_hash(&info.owner_pub_keys_digest.into());
        data_vault.set_vendor_ecc_pk_index(info.vendor_ecc_pub_key_idx);
        data_vault.set_vendor_pqc_pk_index(info.vendor_pqc_pub_key_idx);
        data_vault.set_rt_tci(&info.runtime.digest.into());
        data_vault.set_fw_svn(info.fw_svn);
        data_vault.set_fw_min_svn(info.fw_svn);
        data_vault.set_rt_entry_point(info.runtime.entry_point);
        data_vault.set_manifest_addr(manifest_address);

        report_boot_status(FwProcessorPopulateDataVaultComplete.into());
    }

    #[cfg_attr(not(feature = "no-cfi"), cfi_impl_fn)]
    fn populate_fw_key_ladder(env: &mut RomEnv) -> CaliptraResult<()> {
        let svn = env.persistent_data.get().data_vault.fw_svn();

        if svn > MAX_FIRMWARE_SVN {
            // If this occurs it is an internal programming error.
            Err(CaliptraError::FW_PROC_SVN_TOO_LARGE)?;
        }

        let chain_len = MAX_FIRMWARE_SVN - svn;

        cprintln!(
            "[fwproc] Initializing chain, length {} (max {})",
            chain_len,
            MAX_FIRMWARE_SVN
        );

        key_ladder::initialize_key_ladder(env, chain_len)?;

        cprintln!("[fwproc] Chain initialized");

        report_boot_status(FwProcessorCalculateKeyLadderComplete.into());

        Ok(())
    }

    /// Process the certificate validity info
    ///
    /// # Arguments
    /// * `manifest` - Manifest
    ///
    /// # Returns
    /// * `NotBefore` - Valid Not Before Time
    /// * `NotAfter`  - Valid Not After Time
    ///
    fn get_cert_validity_info(manifest: &ImageManifest) -> (NotBefore, NotAfter) {
        // If there is a valid value in the manifest for the not_before and not_after times,
        // use those. Otherwise use the default values.
        let mut nb = NotBefore::default();
        let mut nf = NotAfter::default();
        let null_time = [0u8; 15];

        if manifest.header.vendor_data.vendor_not_after != null_time
            && manifest.header.vendor_data.vendor_not_before != null_time
        {
            nf.value = manifest.header.vendor_data.vendor_not_after;
            nb.value = manifest.header.vendor_data.vendor_not_before;
        }

        // Owner values take preference.
        if manifest.header.owner_data.owner_not_after != null_time
            && manifest.header.owner_data.owner_not_before != null_time
        {
            nf.value = manifest.header.owner_data.owner_not_after;
            nb.value = manifest.header.owner_data.owner_not_before;
        }

        (nb, nf)
    }

    /// Read request from mailbox and verify the checksum
    ///
    /// # Arguments
    /// * `txn` - Mailbox Receive Transaction
    /// * `data` - Data buffer for the expected request
    ///
    /// # Returns
    /// * `()` - Ok
    ///    Error code on failure.
    pub fn copy_req_verify_chksum(txn: &mut MailboxRecvTxn, data: &mut [u8]) -> CaliptraResult<()> {
        // NOTE: Currently ROM only supports commands with a fixed request size
        //       This check will need to be updated if any commands are added with a variable request size
        if txn.dlen() as usize != data.len() {
            return Err(CaliptraError::FW_PROC_MAILBOX_INVALID_REQUEST_LENGTH);
        }

        // Read the data in from the mailbox HW
        txn.copy_request(data)?;

        // Extract header out from the rest of the request
        let req_hdr =
            MailboxReqHeader::ref_from_bytes(&data[..core::mem::size_of::<MailboxReqHeader>()])
                .map_err(|_| CaliptraError::FW_PROC_MAILBOX_PROCESS_FAILURE)?;

        // Verify checksum
        if !caliptra_common::checksum::verify_checksum(
            req_hdr.chksum,
            txn.cmd(),
            &data[core::mem::size_of_val(&req_hdr.chksum)..],
        ) {
            return Err(CaliptraError::FW_PROC_MAILBOX_INVALID_CHECKSUM);
        };

        Ok(())
    }

    /// Read measurement from mailbox and extends it into PCR31
    ///
    /// # Arguments
    /// * `pcr_bank` - PCR Bank
    /// * `sha384` - SHA384
    /// * `persistent_data` - Persistent data
    /// * `txn` - Mailbox Receive Transaction
    ///
    /// # Returns
    /// * `()` - Ok
    ///     Err - StashMeasurementReadFailure
    fn stash_measurement(
        pcr_bank: &mut PcrBank,
        sha2: &mut Sha2_512_384,
        persistent_data: &mut PersistentData,
        txn: &mut MailboxRecvTxn,
    ) -> CaliptraResult<()> {
        let mut measurement = StashMeasurementReq::default();
        Self::copy_req_verify_chksum(txn, measurement.as_mut_bytes())?;

        // Extend measurement into PCR31.
        Self::extend_measurement(pcr_bank, sha2, persistent_data, &measurement)?;

        Ok(())
    }

    /// Extends measurement into PCR31 and logs it to PCR log.
    ///
    /// # Arguments
    /// * `pcr_bank` - PCR Bank
    /// * `sha384` - SHA384
    /// * `persistent_data` - Persistent data
    /// * `stash_measurement` - Measurement
    ///
    /// # Returns
    /// * `()` - Ok
    ///    Error code on failure.
    fn extend_measurement(
        pcr_bank: &mut PcrBank,
        sha2: &mut Sha2_512_384,
        persistent_data: &mut PersistentData,
        stash_measurement: &StashMeasurementReq,
    ) -> CaliptraResult<()> {
        // Extend measurement into PCR31.
        pcr_bank.extend_pcr(
            PCR_ID_STASH_MEASUREMENT,
            sha2,
            stash_measurement.measurement.as_bytes(),
        )?;

        // Log measurement to the measurement log.
        Self::log_measurement(persistent_data, stash_measurement)
    }

    /// Log measurement data to the Stash Measurement log
    ///
    /// # Arguments
    /// * `persistent_data` - Persistent data
    /// * `stash_measurement` - Measurement
    ///
    /// # Return Value
    /// * `Ok(())` - Success
    /// * `Err(GlobalErr::MeasurementLogExhausted)` - Measurement log exhausted
    ///
    pub fn log_measurement(
        persistent_data: &mut PersistentData,
        stash_measurement: &StashMeasurementReq,
    ) -> CaliptraResult<()> {
        let fht = &mut persistent_data.fht;
        let Some(dst) = persistent_data
            .measurement_log
            .get_mut(fht.meas_log_index as usize)
        else {
            return Err(CaliptraError::ROM_GLOBAL_MEASUREMENT_LOG_EXHAUSTED);
        };

        *dst = MeasurementLogEntry {
            pcr_entry: PcrLogEntry {
                id: PcrLogEntryId::StashMeasurement as u16,
                reserved0: [0u8; 2],
                pcr_ids: 1 << (PCR_ID_STASH_MEASUREMENT as u8),
                pcr_data: zerocopy::transmute!(stash_measurement.measurement),
            },
            metadata: stash_measurement.metadata,
            context: zerocopy::transmute!(stash_measurement.context),
            svn: stash_measurement.svn,
            reserved0: [0u8; 4],
        };

        fht.meas_log_index += 1;

        Ok(())
    }

    /// Retrieve the fw image from the recovery interface and store it in the mailbox sram.
    ///
    /// # Arguments
    /// * `dma` - DMA driver
    /// * `soc_ifc` - SOC Interface
    ///
    /// # Returns
    /// * `()` - Ok
    ///   Error code on failure.
    fn retrieve_image_from_recovery_interface(
        dma: &mut Dma,
        soc_ifc: &mut SocIfc,
    ) -> CaliptraResult<u32> {
        let rri_base_addr = soc_ifc.recovery_interface_base_addr().into();
        const FW_IMAGE_INDEX: u32 = 0x0;
        let dma_recovery = DmaRecovery::new(rri_base_addr, dma);
        dma_recovery.download_image_to_mbox(FW_IMAGE_INDEX)
    }
}
