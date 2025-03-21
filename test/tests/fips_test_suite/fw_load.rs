// Licensed under the Apache-2.0 license
use crate::common;

use caliptra_api::SocManager;
use caliptra_builder::firmware::{
    APP_WITH_UART, FMC_FAKE_WITH_UART, FMC_WITH_UART, ROM_WITH_FIPS_TEST_HOOKS,
};
use caliptra_builder::ImageOptions;
use caliptra_common::memory_layout::{ICCM_ORG, ICCM_SIZE};
use caliptra_drivers::CaliptraError;
use caliptra_drivers::FipsTestHook;
use caliptra_hw_model::{
    BootParams, DeviceLifecycle, Fuses, HwModel, InitParams, ModelError, SecurityState, U4,
};
use caliptra_image_crypto::OsslCrypto as Crypto;
use caliptra_image_fake_keys::{VENDOR_CONFIG_KEY_0, VENDOR_CONFIG_KEY_1};
use caliptra_image_gen::{ImageGenerator, ImageGeneratorConfig, ImageGeneratorVendorConfig};
use caliptra_image_types::{
    FwVerificationPqcKeyType, ImageBundle, ImageDigestHolder, ImageLmsPublicKey, ImageMldsaPubKey,
    MLDSA87_PUB_KEY_WORD_SIZE, SHA384_DIGEST_WORD_SIZE, VENDOR_ECC_MAX_KEY_COUNT,
    VENDOR_LMS_MAX_KEY_COUNT, VENDOR_MLDSA_MAX_KEY_COUNT,
};
use caliptra_test::image_pk_desc_hash;

use common::*;
use zerocopy::{FromBytes, IntoBytes};

#[allow(dead_code)]
#[derive(PartialEq, Eq)]
enum HdrDigest {
    Update,
    Skip,
}

#[derive(PartialEq, Eq)]
enum TocDigest {
    Update,
    Skip,
}

const PQC_KEY_TYPE: [FwVerificationPqcKeyType; 2] = [
    FwVerificationPqcKeyType::MLDSA,
    FwVerificationPqcKeyType::LMS,
];

pub fn build_fw_image(image_options: ImageOptions) -> ImageBundle {
    caliptra_builder::build_and_sign_image(&FMC_WITH_UART, &APP_WITH_UART, image_options).unwrap()
}

fn update_manifest(image_bundle: &mut ImageBundle, hdr_digest: HdrDigest, toc_digest: TocDigest) {
    let pqc_key_type =
        FwVerificationPqcKeyType::from_u8(image_bundle.manifest.pqc_key_type).unwrap();
    let opts = ImageOptions {
        pqc_key_type,
        ..Default::default()
    };
    let config = ImageGeneratorConfig {
        fmc: caliptra_image_elf::ElfExecutable::default(),
        runtime: caliptra_image_elf::ElfExecutable::default(),
        vendor_config: opts.vendor_config,
        owner_config: opts.owner_config,
        pqc_key_type,
        fw_svn: 0,
    };

    let gen = ImageGenerator::new(Crypto::default());

    // Update TOC digest
    if toc_digest == TocDigest::Update {
        image_bundle.manifest.header.toc_digest = gen
            .toc_digest(&image_bundle.manifest.fmc, &image_bundle.manifest.runtime)
            .unwrap();
    }

    if hdr_digest == HdrDigest::Update {
        let vendor_header_digest_384 = gen
            .vendor_header_digest_384(&image_bundle.manifest.header)
            .unwrap();
        let vendor_header_digest_512 = gen
            .vendor_header_digest_512(&image_bundle.manifest.header)
            .unwrap();
        let vendor_header_digest_holder = ImageDigestHolder {
            digest_384: &vendor_header_digest_384,
            digest_512: Some(&vendor_header_digest_512),
        };

        let owner_header_digest_384 = gen
            .owner_header_digest_384(&image_bundle.manifest.header)
            .unwrap();
        let owner_header_digest_512 = gen
            .owner_header_digest_512(&image_bundle.manifest.header)
            .unwrap();
        let owner_header_digest_holder = ImageDigestHolder {
            digest_384: &owner_header_digest_384,
            digest_512: Some(&owner_header_digest_512),
        };

        // Update preamble
        image_bundle.manifest.preamble = gen
            .gen_preamble(
                &config,
                image_bundle.manifest.preamble.vendor_ecc_pub_key_idx,
                image_bundle.manifest.preamble.vendor_pqc_pub_key_idx,
                &vendor_header_digest_holder,
                &owner_header_digest_holder,
            )
            .unwrap();
    }
}

// Get a byte array from an image_bundle without any error checking
// Normally, to_bytes will perform some error checking
// We need to bypass this for the sake of these tests
fn image_to_bytes_no_error_check(image_bundle: &ImageBundle) -> Vec<u8> {
    let mut image = vec![];
    image.extend_from_slice(image_bundle.manifest.as_bytes());
    image.extend_from_slice(&image_bundle.fmc);
    image.extend_from_slice(&image_bundle.runtime);
    image
}

// Returns a fuse struct with safe values for boot
// (Mainly needed for manufacturing or production security states)
fn safe_fuses(fw_image: &ImageBundle) -> Fuses {
    let gen = ImageGenerator::new(Crypto::default());

    let vendor_pubkey_digest = gen
        .vendor_pubkey_digest(&fw_image.manifest.preamble)
        .unwrap();

    let owner_pubkey_digest = gen
        .owner_pubkey_digest(&fw_image.manifest.preamble)
        .unwrap();

    Fuses {
        vendor_pk_hash: vendor_pubkey_digest,
        owner_pk_hash: owner_pubkey_digest,
        fuse_pqc_key_type: fw_image.manifest.pqc_key_type as u32,
        ..Default::default()
    }
}

// NOTE: These tests are about the image verification which is contained in ROM.
//       The version of the FW used in the image bundles within these tests is irrelevant.
//       Because of this, we are just building the FW so it's easier to modify components
//       of the image bundle instead of using any pre-existing FW binary

fn fw_load_error_flow(
    fw_image: Option<ImageBundle>,
    fuses: Option<Fuses>,
    exp_error_code: u32,
    pqc_key_type: FwVerificationPqcKeyType,
) {
    fw_load_error_flow_base(
        fw_image,
        None,
        fuses,
        None,
        exp_error_code,
        None,
        pqc_key_type,
    );
}

fn fw_load_error_flow_with_test_hooks(
    fw_image: Option<ImageBundle>,
    fuses: Option<Fuses>,
    exp_error_code: u32,
    test_hook_cmd: u8,
    pqc_key_type: FwVerificationPqcKeyType,
) {
    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_FIPS_TEST_HOOKS).unwrap();
    fw_load_error_flow_base(
        fw_image,
        Some(&rom),
        fuses,
        None,
        exp_error_code,
        Some((test_hook_cmd as u32) << HOOK_CODE_OFFSET),
        pqc_key_type,
    );
}

fn update_fw_error_flow(
    fw_image: Option<ImageBundle>,
    fuses: Option<Fuses>,
    update_fw_image: Option<ImageBundle>,
    exp_error_code: u32,
    pqc_key_type: FwVerificationPqcKeyType,
) {
    let update_fw_image = update_fw_image.unwrap_or(build_fw_image(ImageOptions::default()));

    fw_load_error_flow_base(
        fw_image,
        None,
        fuses,
        Some(update_fw_image),
        exp_error_code,
        None,
        pqc_key_type,
    );
}

fn fw_load_error_flow_base(
    fw_image: Option<ImageBundle>,
    rom: Option<&[u8]>,
    fuses: Option<Fuses>,
    update_fw_image: Option<ImageBundle>,
    exp_error_code: u32,
    initial_dbg_manuf_service_reg: Option<u32>,
    pqc_key_type: FwVerificationPqcKeyType,
) {
    // Use defaults if not provided
    let fuses_default = Fuses {
        fuse_pqc_key_type: pqc_key_type as u32,
        ..Default::default()
    };
    let fuses = fuses.unwrap_or(fuses_default);
    let image_options = ImageOptions {
        pqc_key_type,
        ..Default::default()
    };
    let fw_image = fw_image.unwrap_or(build_fw_image(image_options.clone()));

    // Attempt to load the FW
    let mut hw = fips_test_init_to_rom(
        Some(InitParams {
            security_state: SecurityState::from(fuses.life_cycle as u32),
            rom: rom.unwrap_or_default(),
            ..Default::default()
        }),
        Some(BootParams {
            fuses,
            initial_dbg_manuf_service_reg: initial_dbg_manuf_service_reg.unwrap_or_default(),
            ..Default::default()
        }),
    );

    // Upload initial FW
    let mut fw_load_result = hw.upload_firmware(&image_to_bytes_no_error_check(&fw_image));

    // Update the FW if specified
    match update_fw_image {
        None => {
            // Verify the correct error was returned from FW load
            assert_eq!(
                ModelError::MailboxCmdFailed(exp_error_code),
                fw_load_result.unwrap_err()
            );

            // Verify we cannot utilize RT FW by sending a message
            verify_mbox_cmds_fail(&mut hw, exp_error_code);

            // Verify an undocumented attempt to clear the error fails
            hw.soc_ifc().cptra_fw_error_fatal().write(|_| 0);
            hw.soc_ifc().cptra_fw_error_non_fatal().write(|_| 0);
            verify_mbox_cmds_fail(&mut hw, 0);

            // Clear the error with an approved method - restart Caliptra
            // TODO: Reset to the default fuse state - provided fuses may be intended to cause errors
            if cfg!(any(feature = "verilator", feature = "fpga_realtime")) {
                hw.cold_reset();
            } else {
                hw = fips_test_init_model(None)
            }

            let clean_fw_image = build_fw_image(image_options);

            hw.boot(BootParams {
                fuses: safe_fuses(&clean_fw_image),
                ..Default::default()
            })
            .unwrap();

            hw.step_until(|m| {
                m.soc_ifc()
                    .cptra_flow_status()
                    .read()
                    .ready_for_mb_processing()
            });

            // Verify we can load FW (use clean FW)
            hw.upload_firmware(&clean_fw_image.to_bytes().unwrap())
                .unwrap();
        }
        Some(update_image) => {
            // Verify initial FW load was successful
            fw_load_result.unwrap();

            // Update FW
            fw_load_result = hw.upload_firmware(&image_to_bytes_no_error_check(&update_image));
            // Verify the correct error was returned from FW load
            assert_eq!(
                fw_load_result.unwrap_err(),
                ModelError::MailboxCmdFailed(exp_error_code)
            );

            // In the update FW case, the error will be non-fatal and fall back to the previous, good FW

            // Verify we can load FW (use first FW)
            hw.upload_firmware(&image_to_bytes_no_error_check(&fw_image))
                .unwrap();
        }
    }
}

#[test]
fn fw_load_error_manifest_marker_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Corrupt manifest marker
        fw_image.manifest.marker = 0xDEADBEEF;

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_MANIFEST_MARKER_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_manifest_size_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change manifest size
        fw_image.manifest.size -= 1;

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_MANIFEST_SIZE_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_pub_key_digest_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Set fuses
        let fuses = caliptra_hw_model::Fuses {
            life_cycle: DeviceLifecycle::Manufacturing,
            vendor_pk_hash: [0u32; 12],
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        fw_load_error_flow(
            None,
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_vendor_pub_key_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Set fuses
        let fuses = caliptra_hw_model::Fuses {
            life_cycle: DeviceLifecycle::Manufacturing,
            vendor_pk_hash: [0xDEADBEEF; 12],
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        fw_load_error_flow_with_test_hooks(
            None,
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_FAILURE.into(),
            FipsTestHook::FW_LOAD_VENDOR_PUB_KEY_DIGEST_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_pub_key_digest_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Set fuses
        let fuses = caliptra_hw_model::Fuses {
            life_cycle: DeviceLifecycle::Manufacturing,
            vendor_pk_hash: [0xDEADBEEF; 12],
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        fw_load_error_flow(
            None,
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_owner_pub_key_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_FAILURE.into(),
            FipsTestHook::FW_LOAD_OWNER_PUB_KEY_DIGEST_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_owner_pub_key_digest_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Set fuses
        let fuses = caliptra_hw_model::Fuses {
            owner_pk_hash: [0xDEADBEEF; 12],
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        fw_load_error_flow(
            None,
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_ecc_pub_key_index_out_of_bounds() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        // Generate image
        let mut fw_image = build_fw_image(image_options);
        // Change ECC pub key index to max+1
        fw_image.manifest.preamble.vendor_ecc_pub_key_idx = VENDOR_ECC_MAX_KEY_COUNT;

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INDEX_OUT_OF_BOUNDS.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_ecc_pub_key_revoked() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let vendor_config = VENDOR_CONFIG_KEY_1;
        let image_options = ImageOptions {
            vendor_config,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };

        // Set fuses
        let fuses = caliptra_hw_model::Fuses {
            fuse_ecc_revocation: U4::try_from(1u32 << image_options.vendor_config.ecc_key_idx)
                .unwrap(),
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        // Generate image
        let fw_image = build_fw_image(image_options);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_REVOKED.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_header_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_HEADER_DIGEST_FAILURE.into(),
            FipsTestHook::FW_LOAD_HEADER_DIGEST_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_vendor_ecc_verify_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_VERIFY_FAILURE.into(),
            FipsTestHook::FW_LOAD_VENDOR_ECC_VERIFY_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_ecc_signature_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Corrupt vendor ECC sig
        fw_image.manifest.preamble.vendor_sigs.ecc_sig.r.fill(1);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_ecc_pub_key_index_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change vendor pubkey index.
        fw_image.manifest.header.vendor_ecc_pub_key_idx =
            fw_image.manifest.preamble.vendor_ecc_pub_key_idx + 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INDEX_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_owner_ecc_verify_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_VERIFY_FAILURE.into(),
            FipsTestHook::FW_LOAD_OWNER_ECC_VERIFY_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_owner_ecc_signature_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Corrupt owner ECC sig
        fw_image.manifest.preamble.owner_sigs.ecc_sig.r.fill(1);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_SIGNATURE_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_toc_entry_count_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change the TOC length to over the maximum
        fw_image.manifest.header.toc_len = caliptra_image_types::MAX_TOC_ENTRY_COUNT + 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_TOC_ENTRY_COUNT_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_toc_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_TOC_DIGEST_FAILURE.into(),
            FipsTestHook::FW_LOAD_OWNER_TOC_DIGEST_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_toc_digest_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change the TOC digest.
        fw_image.manifest.header.toc_digest[0] = 0xDEADBEEF;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Skip);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_TOC_DIGEST_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_fmc_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_DIGEST_FAILURE.into(),
            FipsTestHook::FW_LOAD_FMC_DIGEST_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_digest_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change the FMC image.
        fw_image.fmc[0..4].copy_from_slice(0xDEADBEEFu32.as_bytes());

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_DIGEST_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_runtime_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        fw_load_error_flow_with_test_hooks(
            None,
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_DIGEST_FAILURE.into(),
            FipsTestHook::FW_LOAD_RUNTIME_DIGEST_FAILURE,
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_digest_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change the runtime image.
        fw_image.runtime[0..4].copy_from_slice(0xDEADBEEFu32.as_bytes());

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_DIGEST_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_runtime_overlap() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Corrupt FMC offset
        fw_image.manifest.fmc.offset = fw_image.manifest.runtime.offset;

        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_OVERLAP.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_runtime_incorrect_order() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Flip FMC and RT positions
        let old_fmc_offset = fw_image.manifest.fmc.offset;
        let old_fmc_size = fw_image.manifest.fmc.size;
        fw_image.manifest.fmc.offset = fw_image.manifest.runtime.offset;
        fw_image.manifest.fmc.size = fw_image.manifest.runtime.size;
        fw_image.manifest.runtime.offset = old_fmc_offset;
        fw_image.manifest.runtime.size = old_fmc_size;

        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_INCORRECT_ORDER.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_owner_ecc_pub_key_invalid_arg() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Set ecc_pub_key.y to zero.
        fw_image
            .manifest
            .preamble
            .owner_pub_keys
            .ecc_pub_key
            .y
            .fill(0);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_PUB_KEY_INVALID_ARG.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_owner_ecc_signature_invalid_arg() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Set owner_sig.s to zero.
        fw_image.manifest.preamble.owner_sigs.ecc_sig.s.fill(0);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_ECC_SIGNATURE_INVALID_ARG.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_pub_key_invalid_arg() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Set ecc_pub_key.x to zero.
        fw_image
            .manifest
            .preamble
            .vendor_ecc_active_pub_key
            .x
            .fill(0);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_PUB_KEY_INVALID_ARG.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_ecc_signature_invalid_arg() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Set vendor_sig.r to zero.
        fw_image.manifest.preamble.vendor_sigs.ecc_sig.r.fill(0);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_ECC_SIGNATURE_INVALID_ARG.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_update_reset_owner_digest_failure() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut update_image = build_fw_image(image_options);

        // Set ecc_pub_key.y to some corrupted, non-zero value
        update_image
            .manifest
            .preamble
            .owner_pub_keys
            .ecc_pub_key
            .y
            .fill(0x1234abcd);

        update_fw_error_flow(
            None,
            None,
            Some(update_image),
            CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_OWNER_DIGEST_FAILURE.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_update_reset_vendor_ecc_pub_key_idx_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let vendor_config_cold_boot = ImageGeneratorVendorConfig {
            ecc_key_idx: 3,
            ..VENDOR_CONFIG_KEY_0
        };
        let image_options_cold_boot = ImageOptions {
            vendor_config: vendor_config_cold_boot,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let vendor_config_update_reset = ImageGeneratorVendorConfig {
            ecc_key_idx: 2,
            ..VENDOR_CONFIG_KEY_0
        };
        let image_options_update_reset = ImageOptions {
            vendor_config: vendor_config_update_reset,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        // Generate images
        let first_image = build_fw_image(image_options_cold_boot);
        let update_image = build_fw_image(image_options_update_reset);

        update_fw_error_flow(
            Some(first_image),
            None,
            Some(update_image),
            CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_VENDOR_ECC_PUB_KEY_IDX_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_update_reset_fmc_digest_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate images
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let first_image = build_fw_image(image_options.clone());
        // Use a different FMC for the update image
        let update_image = caliptra_builder::build_and_sign_image(
            &FMC_FAKE_WITH_UART,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        update_fw_error_flow(
            Some(first_image),
            None,
            Some(update_image),
            CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_FMC_DIGEST_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_load_addr_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change FMC load addr
        fw_image.manifest.fmc.load_addr = ICCM_ORG - 4;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDR_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_load_addr_unaligned() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change FMC load addr
        fw_image.manifest.fmc.load_addr += 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDR_UNALIGNED.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_entry_point_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change FMC entry point
        fw_image.manifest.fmc.entry_point = ICCM_ORG - 4;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_ENTRY_POINT_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_entry_point_unaligned() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change FMC entry point
        fw_image.manifest.fmc.entry_point += 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_ENTRY_POINT_UNALIGNED.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_load_addr_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime load addr
        fw_image.manifest.runtime.load_addr = ICCM_ORG + ICCM_SIZE;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDR_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_load_addr_unaligned() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime load addr
        fw_image.manifest.runtime.load_addr += 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDR_UNALIGNED.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_entry_point_invalid() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime entry point
        fw_image.manifest.runtime.entry_point = ICCM_ORG - 4;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_ENTRY_POINT_INVALID.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_entry_point_unaligned() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime entry point
        fw_image.manifest.runtime.entry_point += 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_ENTRY_POINT_UNALIGNED.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_svn_greater_than_max_supported() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            fw_svn: caliptra_image_verify::MAX_FIRMWARE_SVN + 1,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fw_image = build_fw_image(image_options);

        // Set fuses
        let gen = ImageGenerator::new(Crypto::default());
        let vendor_pubkey_digest = gen
            .vendor_pubkey_digest(&fw_image.manifest.preamble)
            .unwrap();
        let fuses = caliptra_hw_model::Fuses {
            life_cycle: DeviceLifecycle::Manufacturing,
            anti_rollback_disable: false,
            vendor_pk_hash: vendor_pubkey_digest,
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FIRMWARE_SVN_GREATER_THAN_MAX_SUPPORTED.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_svn_less_than_fuse() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            fw_svn: 62,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fw_image = build_fw_image(image_options);

        // Set fuses
        let gen = ImageGenerator::new(Crypto::default());
        let vendor_pubkey_digest = gen
            .vendor_pubkey_digest(&fw_image.manifest.preamble)
            .unwrap();
        let fuses = caliptra_hw_model::Fuses {
            life_cycle: DeviceLifecycle::Manufacturing,
            anti_rollback_disable: false,
            vendor_pk_hash: vendor_pubkey_digest,
            fw_svn: [0xffff_ffff, 0x7fff_ffff, 0, 0], // fuse svn = 63
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_FIRMWARE_SVN_LESS_THAN_FUSE.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_image_len_more_than_bundle_size() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let fuses = caliptra_hw_model::Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime size to exceed bundle
        fw_image.manifest.runtime.size += 4;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            Some(fuses),
            CaliptraError::IMAGE_VERIFIER_ERR_IMAGE_LEN_MORE_THAN_BUNDLE_SIZE.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_vendor_pub_key_index_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change vendor pubkey index.
        fw_image.manifest.header.vendor_pqc_pub_key_idx =
            fw_image.manifest.preamble.vendor_pqc_pub_key_idx + 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_vendor_lms_verify_failure() {
    fw_load_error_flow_with_test_hooks(
        None,
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_LMS_VERIFY_FAILURE.into(),
        FipsTestHook::FW_LOAD_VENDOR_LMS_VERIFY_FAILURE,
        FwVerificationPqcKeyType::LMS,
    );
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_vendor_mldsa_verify_failure() {
    fw_load_error_flow_with_test_hooks(
        None,
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_MLDSA_VERIFY_FAILURE.into(),
        FipsTestHook::FW_LOAD_VENDOR_MLDSA_VERIFY_FAILURE,
        FwVerificationPqcKeyType::MLDSA,
    );
}

#[test]
fn fw_load_error_vendor_lms_pub_key_index_out_of_bounds() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::LMS,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);
    // Set LMS pub key index to MAX + 1
    fw_image.manifest.preamble.vendor_pqc_pub_key_idx = VENDOR_LMS_MAX_KEY_COUNT;

    fw_load_error_flow(
        Some(fw_image),
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_OUT_OF_BOUNDS.into(),
        FwVerificationPqcKeyType::LMS,
    );
}

#[test]
fn fw_load_error_vendor_mldsa_pub_key_index_out_of_bounds() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::MLDSA,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);
    // Set pub key index to MAX + 1
    fw_image.manifest.preamble.vendor_pqc_pub_key_idx = VENDOR_MLDSA_MAX_KEY_COUNT;

    fw_load_error_flow(
        Some(fw_image),
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_INDEX_OUT_OF_BOUNDS.into(),
        FwVerificationPqcKeyType::MLDSA,
    );
}

#[test]
fn fw_load_error_vendor_lms_signature_invalid() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::LMS,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);

    // Get a mutable reference to the LMS public key.
    let (lms_pub_key, _) = ImageLmsPublicKey::mut_from_prefix(
        fw_image
            .manifest
            .preamble
            .vendor_pqc_active_pub_key
            .0
            .as_mut_bytes(),
    )
    .unwrap();

    // Modify the vendor public key.
    lms_pub_key.digest = [Default::default(); 6];

    fw_load_error_flow(
        Some(fw_image),
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_LMS_SIGNATURE_INVALID.into(),
        FwVerificationPqcKeyType::LMS,
    );
}

#[test]
fn fw_load_error_vendor_mldsa_signature_invalid() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::MLDSA,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);

    // Get a mutable reference to the public key.
    let (pub_key, _) = ImageMldsaPubKey::mut_from_prefix(
        fw_image
            .manifest
            .preamble
            .vendor_pqc_active_pub_key
            .0
            .as_mut_bytes(),
    )
    .unwrap();

    // Modify the vendor public key.
    *pub_key = ImageMldsaPubKey([0xDEADBEEF; MLDSA87_PUB_KEY_WORD_SIZE]);

    fw_load_error_flow(
        Some(fw_image),
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_MLDSA_SIGNATURE_INVALID.into(),
        FwVerificationPqcKeyType::MLDSA,
    );
}

#[test]
fn fw_load_error_fmc_runtime_load_addr_overlap() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime entry point
        fw_image.manifest.runtime.load_addr = fw_image.manifest.fmc.load_addr + 1;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_RUNTIME_LOAD_ADDR_OVERLAP.into(),
            *pqc_key_type,
        );
    }
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_owner_lms_verify_failure() {
    fw_load_error_flow_with_test_hooks(
        None,
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_OWNER_LMS_VERIFY_FAILURE.into(),
        FipsTestHook::FW_LOAD_OWNER_LMS_VERIFY_FAILURE,
        FwVerificationPqcKeyType::LMS,
    );
}

#[test]
#[cfg(not(feature = "test_env_immutable_rom"))]
fn fw_load_error_owner_mldsa_verify_failure() {
    fw_load_error_flow_with_test_hooks(
        None,
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_OWNER_MLDSA_VERIFY_FAILURE.into(),
        FipsTestHook::FW_LOAD_OWNER_MLDSA_VERIFY_FAILURE,
        FwVerificationPqcKeyType::MLDSA,
    );
}

#[test]
fn fw_load_error_owner_lms_signature_invalid() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::LMS,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);

    // Get a mutable reference to the LMS public key.
    let (lms_pub_key, _) = ImageLmsPublicKey::mut_from_prefix(
        fw_image
            .manifest
            .preamble
            .owner_pub_keys
            .pqc_pub_key
            .0
            .as_mut_bytes(),
    )
    .unwrap();

    // Modify the owner public key
    lms_pub_key.digest = [Default::default(); 6];

    fw_load_error_flow(
        Some(fw_image),
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_OWNER_LMS_SIGNATURE_INVALID.into(),
        FwVerificationPqcKeyType::LMS,
    );
}

#[test]
fn fw_load_error_owner_mldsa_signature_invalid() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::MLDSA,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);

    // Get a mutable reference to the public key.
    let (pub_key, _) = ImageMldsaPubKey::mut_from_prefix(
        fw_image
            .manifest
            .preamble
            .owner_pub_keys
            .pqc_pub_key
            .0
            .as_mut_bytes(),
    )
    .unwrap();

    // Modify the owner public key.
    *pub_key = Default::default();

    fw_load_error_flow(
        Some(fw_image),
        None,
        CaliptraError::IMAGE_VERIFIER_ERR_OWNER_MLDSA_SIGNATURE_INVALID.into(),
        FwVerificationPqcKeyType::MLDSA,
    );
}

#[test]
fn fw_load_error_vendor_lms_pub_key_revoked() {
    let vendor_config = ImageGeneratorVendorConfig {
        pqc_key_idx: 5,
        ..VENDOR_CONFIG_KEY_0
    };
    let image_options = ImageOptions {
        vendor_config,
        pqc_key_type: FwVerificationPqcKeyType::LMS,
        ..Default::default()
    };

    // Set fuses
    let fuses = caliptra_hw_model::Fuses {
        fuse_lms_revocation: 1u32 << image_options.vendor_config.pqc_key_idx,
        fuse_pqc_key_type: FwVerificationPqcKeyType::LMS as u32,
        ..Default::default()
    };

    // Generate image
    let fw_image = build_fw_image(image_options);

    fw_load_error_flow(
        Some(fw_image),
        Some(fuses),
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_REVOKED.into(),
        FwVerificationPqcKeyType::LMS,
    );
}

#[test]
fn fw_load_error_vendor_mldsa_pub_key_revoked() {
    let vendor_config = ImageGeneratorVendorConfig {
        pqc_key_idx: 2,
        ..VENDOR_CONFIG_KEY_0
    };
    let image_options = ImageOptions {
        vendor_config,
        pqc_key_type: FwVerificationPqcKeyType::MLDSA,
        ..Default::default()
    };

    // Set fuses
    let fuses = caliptra_hw_model::Fuses {
        fuse_mldsa_revocation: 1u32 << image_options.vendor_config.pqc_key_idx,
        fuse_pqc_key_type: FwVerificationPqcKeyType::MLDSA as u32,
        ..Default::default()
    };

    // Generate image
    let fw_image = build_fw_image(image_options);

    fw_load_error_flow(
        Some(fw_image),
        Some(fuses),
        CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PQC_PUB_KEY_REVOKED.into(),
        FwVerificationPqcKeyType::MLDSA,
    );
}

#[test]
fn fw_load_error_fmc_size_zero() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change FMC size to 0
        fw_image.manifest.fmc.size = 0;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_SIZE_ZERO.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_size_zero() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime size to 0
        fw_image.manifest.runtime.size = 0;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_SIZE_ZERO.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_update_reset_vendor_pqc_pub_key_idx_mismatch() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let vendor_config_update_reset = ImageGeneratorVendorConfig {
            pqc_key_idx: 2,
            ..VENDOR_CONFIG_KEY_0
        };
        let image_options_update_reset = ImageOptions {
            vendor_config: vendor_config_update_reset,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        // Generate image
        let update_image = build_fw_image(image_options_update_reset);

        update_fw_error_flow(
            None,
            None,
            Some(update_image),
            CaliptraError::IMAGE_VERIFIER_ERR_UPDATE_RESET_VENDOR_PQC_PUB_KEY_IDX_MISMATCH.into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_fmc_load_address_image_size_arithmetic_overflow() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change FMC load addr to cause overflow
        fw_image.manifest.fmc.load_addr = 0xFFFFFFF0;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_FMC_LOAD_ADDRESS_IMAGE_SIZE_ARITHMETIC_OVERFLOW
                .into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_runtime_load_address_image_size_arithmetic_overflow() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change runtime load addr to cause overflow
        fw_image.manifest.runtime.load_addr = 0xFFFFFFF0;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_LOAD_ADDRESS_IMAGE_SIZE_ARITHMETIC_OVERFLOW
                .into(),
            *pqc_key_type,
        );
    }
}

#[test]
fn fw_load_error_toc_entry_range_arithmetic_overflow() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change fmc offset to cause overflow
        fw_image.manifest.fmc.offset = 0xFFFFFFF0;
        update_manifest(&mut fw_image, HdrDigest::Update, TocDigest::Update);

        fw_load_error_flow(
            Some(fw_image),
            None,
            CaliptraError::IMAGE_VERIFIER_ERR_TOC_ENTRY_RANGE_ARITHMETIC_OVERFLOW.into(),
            *pqc_key_type,
        );
    }
}

// IMAGE_VERIFIER_ERR_DIGEST_OUT_OF_BOUNDS is not possible if there is no SW bug
// IMAGE_VERIFIER_ERR_IMAGE_LEN_MORE_THAN_BUNDLE_SIZE or an ARITHMETIC_OVERFLOW error would catch this first

fn fw_load_bad_pub_key_flow(fw_image: ImageBundle, exp_error_code: u32) {
    // Generate pub key hashes and set fuses
    // Use a fresh image (will NOT be loaded)
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::from_u8(fw_image.manifest.pqc_key_type).unwrap(),
        ..Default::default()
    };
    let pk_hash_src_image = build_fw_image(image_options);
    let (vendor_pk_desc_hash, owner_pk_hash) = image_pk_desc_hash(&pk_hash_src_image.manifest);

    let fuses = Fuses {
        life_cycle: DeviceLifecycle::Production,
        vendor_pk_hash: vendor_pk_desc_hash,
        owner_pk_hash,
        fuse_pqc_key_type: fw_image.manifest.pqc_key_type as u32,
        ..Default::default()
    };

    // Load the FW
    let mut hw = fips_test_init_to_rom(
        Some(InitParams {
            security_state: SecurityState::from(fuses.life_cycle as u32),
            ..Default::default()
        }),
        Some(BootParams {
            fuses,
            ..Default::default()
        }),
    );
    let fw_load_result = hw.upload_firmware(&image_to_bytes_no_error_check(&fw_image));

    // Make sure we got the right error
    assert_eq!(
        ModelError::MailboxCmdFailed(exp_error_code),
        fw_load_result.unwrap_err()
    );
}

#[test]
fn fw_load_bad_vendor_ecc_pub_key() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);

        // Modify the pub key hash
        fw_image
            .manifest
            .preamble
            .vendor_pub_key_info
            .ecc_key_descriptor
            .key_hash[0][0] ^= 0x1;

        fw_load_bad_pub_key_flow(
            fw_image,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_MISMATCH.into(),
        );
    }
}

#[test]
fn fw_load_bad_owner_ecc_pub_key() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);

        // Modify the pub key
        fw_image.manifest.preamble.owner_pub_keys.ecc_pub_key.x[0] ^= 0x1;

        fw_load_bad_pub_key_flow(
            fw_image,
            CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_MISMATCH.into(),
        );
    }
}

#[test]
fn fw_load_bad_vendor_pqc_pub_key() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);

        // Modify the pub key hash
        fw_image
            .manifest
            .preamble
            .vendor_pub_key_info
            .pqc_key_descriptor
            .key_hash[0][0] ^= 0x1;

        fw_load_bad_pub_key_flow(
            fw_image,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_MISMATCH.into(),
        );
    }
}

#[test]
fn fw_load_bad_owner_lms_pub_key() {
    // Generate image
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::LMS,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);

    // Modify the pub key
    let (lms_pub_key, _) = ImageLmsPublicKey::mut_from_prefix(
        fw_image
            .manifest
            .preamble
            .owner_pub_keys
            .pqc_pub_key
            .0
            .as_mut_bytes(),
    )
    .unwrap();
    lms_pub_key.digest[0] = 0xDEADBEEF.into();

    fw_load_bad_pub_key_flow(
        fw_image,
        CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_MISMATCH.into(),
    );
}

#[test]
fn fw_load_bad_owner_mldsa_pub_key() {
    let image_options = ImageOptions {
        pqc_key_type: FwVerificationPqcKeyType::MLDSA,
        ..Default::default()
    };
    let mut fw_image = build_fw_image(image_options);

    // Modify the pub key
    let (pub_key, _) = ImageMldsaPubKey::mut_from_prefix(
        fw_image
            .manifest
            .preamble
            .owner_pub_keys
            .pqc_pub_key
            .0
            .as_mut_bytes(),
    )
    .unwrap();
    *pub_key = ImageMldsaPubKey([0xDEADBEEF; MLDSA87_PUB_KEY_WORD_SIZE]);

    fw_load_bad_pub_key_flow(
        fw_image,
        CaliptraError::IMAGE_VERIFIER_ERR_OWNER_PUB_KEY_DIGEST_MISMATCH.into(),
    );
}

#[test]
fn fw_load_blank_pub_keys() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);

        // Clear all pub keys
        fw_image
            .manifest
            .preamble
            .vendor_pub_key_info
            .ecc_key_descriptor
            .key_hash = [[0u32; SHA384_DIGEST_WORD_SIZE]; VENDOR_ECC_MAX_KEY_COUNT as usize];
        fw_image.manifest.preamble.owner_pub_keys =
            caliptra_image_types::ImageOwnerPubKeys::default();

        fw_load_bad_pub_key_flow(
            fw_image,
            CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_MISMATCH.into(),
        );
    }
}

#[test]
fn fw_load_blank_pub_key_hashes() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fw_image = build_fw_image(image_options);

        // Don't populate pub key hashes
        let fuses = Fuses {
            life_cycle: DeviceLifecycle::Production,
            fuse_pqc_key_type: fw_image.manifest.pqc_key_type as u32,
            ..Default::default()
        };

        // Load the FW
        let mut hw = fips_test_init_to_rom(
            Some(InitParams {
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            }),
            Some(BootParams {
                fuses,
                ..Default::default()
            }),
        );
        let fw_load_result = hw.upload_firmware(&image_to_bytes_no_error_check(&fw_image));

        // Make sure we got the right error
        assert_eq!(
            ModelError::MailboxCmdFailed(
                CaliptraError::IMAGE_VERIFIER_ERR_VENDOR_PUB_KEY_DIGEST_INVALID.into()
            ),
            fw_load_result.unwrap_err()
        );
    }
}

#[test]
pub fn corrupted_fw_load_version() {
    for pqc_key_type in PQC_KEY_TYPE.iter() {
        let boot_params = BootParams {
            fuses: Fuses {
                fuse_pqc_key_type: *pqc_key_type as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut hw = fips_test_init_to_rom(None, Some(boot_params));

        // Generate image
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let mut fw_image = build_fw_image(image_options);
        // Change the runtime image.
        fw_image.runtime[0..4].copy_from_slice(0xDEADBEEFu32.as_bytes());

        // Get the initial version
        // Normally we would use a command for this, but we cannot issue commands after a fatal error
        // from a failed FW load. We will use the version/rev reg directly instead. (This is the source
        // for the response of the version command)
        let rom_fmc_fw_version_before = hw.soc_ifc().cptra_fw_rev_id().read();

        // Load the FW
        let fw_load_result = hw.upload_firmware(&image_to_bytes_no_error_check(&fw_image));

        // Make sure we got the right error
        let exp_err: u32 = CaliptraError::IMAGE_VERIFIER_ERR_RUNTIME_DIGEST_MISMATCH.into();
        assert_eq!(
            ModelError::MailboxCmdFailed(exp_err),
            fw_load_result.unwrap_err()
        );

        // Make sure we can't use the module
        verify_mbox_cmds_fail(&mut hw, exp_err);

        // Verify version info is unchanged
        assert_eq!(
            rom_fmc_fw_version_before,
            hw.soc_ifc().cptra_fw_rev_id().read()
        );
    }
}
