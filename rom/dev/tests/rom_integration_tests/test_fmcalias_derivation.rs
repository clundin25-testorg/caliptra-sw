// Licensed under the Apache-2.0 license

use caliptra_api::SocManager;
use caliptra_builder::{
    firmware::{
        self,
        rom_tests::{TEST_FMC_INTERACTIVE, TEST_FMC_WITH_UART},
        APP_WITH_UART,
    },
    ImageOptions,
};
use caliptra_common::mailbox_api::{CommandId, MailboxReqHeader, StashMeasurementReq};
use caliptra_common::RomBootStatus::ColdResetComplete;
use caliptra_common::RomBootStatus::*;
use caliptra_common::{FirmwareHandoffTable, FuseLogEntry, FuseLogEntryId};
use caliptra_common::{PcrLogEntry, PcrLogEntryId};
use caliptra_drivers::{pcr_log::MeasurementLogEntry, DataVault, PcrId};
use caliptra_error::CaliptraError;
use caliptra_hw_model::{BootParams, Fuses, HwModel, InitParams, ModelError, SecurityState};
use caliptra_image_crypto::OsslCrypto as Crypto;
use caliptra_image_fake_keys::{OWNER_CONFIG, VENDOR_CONFIG_KEY_1};
use caliptra_image_gen::ImageGenerator;
use caliptra_image_types::{FwVerificationPqcKeyType, IMAGE_BYTE_SIZE};
use caliptra_test::swap_word_bytes;
use openssl::hash::{Hasher, MessageDigest};
use zerocopy::{FromBytes, IntoBytes, TryFromBytes};

use crate::helpers;

const PCR0_AND_PCR1_EXTENDED_ID: u32 = (1 << PcrId::PcrId0 as u8) | (1 << PcrId::PcrId1 as u8);
const PCR31_EXTENDED_ID: u32 = 1 << PcrId::PcrId31 as u8;

#[test]
fn test_zero_firmware_size() {
    let (mut hw, _image_bundle) =
        helpers::build_hw_model_and_image_bundle(Fuses::default(), ImageOptions::default());

    // Zero-sized firmware.
    assert_eq!(
        hw.upload_firmware(&[]).unwrap_err(),
        ModelError::MailboxCmdFailed(u32::from(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE))
    );
    assert_eq!(
        hw.soc_ifc().cptra_fw_error_fatal().read(),
        u32::from(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE)
    );
    assert_eq!(
        hw.soc_ifc().cptra_boot_status().read(),
        u32::from(LDevIdDerivationComplete)
    );
}

#[test]
fn test_firmware_gt_max_size() {
    // Firmware size > 128 KB.

    let (mut hw, _image_bundle) =
        helpers::build_hw_model_and_image_bundle(Fuses::default(), ImageOptions::default());

    // Manually put the oversize data in the mailbox because
    // HwModel::upload_firmware won't let us.
    assert!(!hw.soc_mbox().lock().read().lock());
    hw.soc_mbox()
        .cmd()
        .write(|_| CommandId::FIRMWARE_LOAD.into());
    hw.soc_mbox().dlen().write(|_| (IMAGE_BYTE_SIZE + 1) as u32);
    for i in 0..((IMAGE_BYTE_SIZE + 1 + 3) / 4) {
        hw.soc_mbox().datain().write(|_| i as u32);
    }
    hw.soc_mbox().execute().write(|w| w.execute(true));
    while hw.soc_mbox().status().read().status().cmd_busy() {
        hw.step();
    }
    hw.soc_mbox().execute().write(|w| w.execute(false));

    assert_eq!(
        hw.soc_ifc().cptra_fw_error_fatal().read(),
        u32::from(CaliptraError::FW_PROC_INVALID_IMAGE_SIZE)
    );
    assert_eq!(
        hw.soc_ifc().cptra_boot_status().read(),
        u32::from(LDevIdDerivationComplete)
    );
}

const PCR_COUNT: usize = 32;
const PCR_ENTRY_SIZE: usize = core::mem::size_of::<PcrLogEntry>();
const MEASUREMENT_ENTRY_SIZE: usize = core::mem::size_of::<MeasurementLogEntry>();
const MEASUREMENT_MAX_COUNT: usize = 8;

fn check_pcr_log_entry(
    pcr_entry_arr: &[u8],
    pcr_entry_index: usize,
    entry_id: PcrLogEntryId,
    pcr_ids: u32,
    pcr_data: &[u8],
) {
    let offset = pcr_entry_index * PCR_ENTRY_SIZE;
    let (entry, _) = PcrLogEntry::ref_from_prefix(pcr_entry_arr[offset..].as_bytes()).unwrap();

    assert_eq!(entry.id, entry_id as u16);
    assert_eq!(entry.pcr_ids, pcr_ids);
    assert_eq!(entry.measured_data(), pcr_data);
}

fn check_measurement_log_entry(
    measurement_entry_arr: &[u8],
    measurement_entry_index: usize,
    measurement_req: &StashMeasurementReq,
) {
    let offset = measurement_entry_index * MEASUREMENT_ENTRY_SIZE;
    let (entry, _) =
        MeasurementLogEntry::ref_from_prefix(measurement_entry_arr[offset..].as_bytes()).unwrap();

    assert_eq!(entry.pcr_entry.id, PcrLogEntryId::StashMeasurement as u16);
    assert_eq!(entry.pcr_entry.pcr_ids, PCR31_EXTENDED_ID);
    assert_eq!(
        entry.pcr_entry.measured_data(),
        &measurement_req.measurement
    );
    assert_eq!(entry.metadata, measurement_req.metadata);
    assert_eq!(entry.context.as_bytes(), &measurement_req.context);
    assert_eq!(entry.svn, measurement_req.svn);
}

#[test]
fn test_pcr_log() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let gen = ImageGenerator::new(Crypto::default());
        let image_bundle = helpers::build_image_bundle(image_options);

        let vendor_pubkey_digest = gen
            .vendor_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        let owner_pubkey_digest = gen
            .owner_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        let fuses = Fuses {
            anti_rollback_disable: true,
            vendor_pk_hash: vendor_pubkey_digest,
            owner_pk_hash: owner_pubkey_digest,
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        const FW_SVN: u32 = 1;
        let image_options = ImageOptions {
            vendor_config: VENDOR_CONFIG_KEY_1,
            fw_svn: FW_SVN,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        let pcr_entry_arr = hw.mailbox_execute(0x1000_0000, &[]).unwrap().unwrap();

        let device_lifecycle = hw
            .soc_ifc()
            .cptra_security_state()
            .read()
            .device_lifecycle();

        let debug_locked = hw.soc_ifc().cptra_security_state().read().debug_locked();

        let anti_rollback_disable = hw.soc_ifc().fuse_anti_rollback_disable().read().dis();

        check_pcr_log_entry(
            &pcr_entry_arr,
            0,
            PcrLogEntryId::DeviceStatus,
            PCR0_AND_PCR1_EXTENDED_ID,
            &[
                device_lifecycle as u8,
                debug_locked as u8,
                anti_rollback_disable as u8,
                VENDOR_CONFIG_KEY_1.ecc_key_idx as u8,
                FW_SVN as u8,
                0_u8,
                VENDOR_CONFIG_KEY_1.pqc_key_idx as u8,
                *pqc_key_type as u8,
                true as u8,
            ],
        );

        check_pcr_log_entry(
            &pcr_entry_arr,
            1,
            PcrLogEntryId::VendorPubKeyInfoHash,
            PCR0_AND_PCR1_EXTENDED_ID,
            swap_word_bytes(&vendor_pubkey_digest).as_bytes(),
        );

        check_pcr_log_entry(
            &pcr_entry_arr,
            2,
            PcrLogEntryId::OwnerPubKeyHash,
            PCR0_AND_PCR1_EXTENDED_ID,
            swap_word_bytes(&owner_pubkey_digest).as_bytes(),
        );

        check_pcr_log_entry(
            &pcr_entry_arr,
            3,
            PcrLogEntryId::FmcTci,
            PCR0_AND_PCR1_EXTENDED_ID,
            swap_word_bytes(&image_bundle.manifest.fmc.digest).as_bytes(),
        );
    }
}

#[test]
fn test_pcr_log_no_owner_key_digest_fuse() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let gen = ImageGenerator::new(Crypto::default());
        let image_bundle = helpers::build_image_bundle(image_options);

        let owner_pubkey_digest = gen
            .owner_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        let fuses = Fuses {
            anti_rollback_disable: true,
            vendor_pk_hash: gen
                .vendor_pubkey_digest(&image_bundle.manifest.preamble)
                .unwrap(),
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_options = ImageOptions {
            vendor_config: VENDOR_CONFIG_KEY_1,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        let pcr_entry_arr = hw.mailbox_execute(0x1000_0000, &[]).unwrap().unwrap();

        let device_lifecycle = hw
            .soc_ifc()
            .cptra_security_state()
            .read()
            .device_lifecycle();

        let debug_locked = hw.soc_ifc().cptra_security_state().read().debug_locked();

        let anti_rollback_disable = hw.soc_ifc().fuse_anti_rollback_disable().read().dis();

        check_pcr_log_entry(
            &pcr_entry_arr,
            0,
            PcrLogEntryId::DeviceStatus,
            PCR0_AND_PCR1_EXTENDED_ID,
            &[
                device_lifecycle as u8,
                debug_locked as u8,
                anti_rollback_disable as u8,
                VENDOR_CONFIG_KEY_1.ecc_key_idx as u8,
                0_u8,
                0_u8,
                VENDOR_CONFIG_KEY_1.pqc_key_idx as u8,
                *pqc_key_type as u8,
                false as u8,
            ],
        );

        check_pcr_log_entry(
            &pcr_entry_arr,
            2,
            PcrLogEntryId::OwnerPubKeyHash,
            PCR0_AND_PCR1_EXTENDED_ID,
            swap_word_bytes(&owner_pubkey_digest).as_bytes(),
        );
    }
}

#[test]
fn test_pcr_log_fmc_fuse_svn() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let gen = ImageGenerator::new(Crypto::default());
        let image_bundle = helpers::build_image_bundle(image_options);

        let vendor_pubkey_digest = gen
            .vendor_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        let owner_pubkey_digest = gen
            .owner_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        const FW_SVN: u32 = 3;
        const FW_FUSE_SVN: u32 = 2;

        let fuses = Fuses {
            anti_rollback_disable: false,
            vendor_pk_hash: vendor_pubkey_digest,
            owner_pk_hash: owner_pubkey_digest,
            fw_svn: [0x3, 0, 0, 0], // TODO: add tooling to make this more ergonomic.
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_options = ImageOptions {
            vendor_config: VENDOR_CONFIG_KEY_1,
            fw_svn: FW_SVN,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        let pcr_entry_arr = hw.mailbox_execute(0x1000_0000, &[]).unwrap().unwrap();

        let device_lifecycle = hw
            .soc_ifc()
            .cptra_security_state()
            .read()
            .device_lifecycle();

        let debug_locked = hw.soc_ifc().cptra_security_state().read().debug_locked();

        let anti_rollback_disable = hw.soc_ifc().fuse_anti_rollback_disable().read().dis();

        check_pcr_log_entry(
            &pcr_entry_arr,
            0,
            PcrLogEntryId::DeviceStatus,
            PCR0_AND_PCR1_EXTENDED_ID,
            &[
                device_lifecycle as u8,
                debug_locked as u8,
                anti_rollback_disable as u8,
                VENDOR_CONFIG_KEY_1.ecc_key_idx as u8,
                FW_SVN as u8,
                FW_FUSE_SVN as u8,
                VENDOR_CONFIG_KEY_1.pqc_key_idx as u8,
                *pqc_key_type as u8,
                true as u8,
            ],
        );
    }
}

fn hash_pcr_log_entry(entry: &PcrLogEntry, pcr: &mut [u8; 48]) {
    let mut hasher = Hasher::new(MessageDigest::sha384()).unwrap();
    hasher.update(pcr).unwrap();
    hasher.update(entry.measured_data()).unwrap();
    let digest: &[u8] = &hasher.finish().unwrap();

    pcr.copy_from_slice(digest);
}

// Computes the PCR from the log.
fn hash_pcr_log_entries(initial_pcr: &[u8; 48], pcr_entry_arr: &[u8], pcr_id: PcrId) -> [u8; 48] {
    let mut offset: usize = 0;
    let mut pcr: [u8; 48] = *initial_pcr;

    assert_eq!(pcr_entry_arr.len() % PCR_ENTRY_SIZE, 0);

    loop {
        if offset == pcr_entry_arr.len() {
            break;
        }

        let (entry, _) = PcrLogEntry::ref_from_prefix(pcr_entry_arr[offset..].as_bytes()).unwrap();
        offset += PCR_ENTRY_SIZE;

        if (entry.pcr_ids & (1 << pcr_id as u8)) == 0 {
            continue;
        }

        hash_pcr_log_entry(entry, &mut pcr);
    }

    pcr
}

fn hash_measurement_log_entries(measurement_entry_arr: &[u8]) -> [u8; 48] {
    let mut offset: usize = 0;
    let mut pcr = [0u8; 48];

    assert_eq!(measurement_entry_arr.len() % MEASUREMENT_ENTRY_SIZE, 0);

    loop {
        if offset == measurement_entry_arr.len() {
            break;
        }

        let (entry, _) =
            MeasurementLogEntry::ref_from_prefix(measurement_entry_arr[offset..].as_bytes())
                .unwrap();
        offset += MEASUREMENT_ENTRY_SIZE;

        hash_pcr_log_entry(&entry.pcr_entry, &mut pcr);
    }

    pcr
}

#[test]
fn test_pcr_log_across_update_reset() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let gen = ImageGenerator::new(Crypto::default());
        let image_bundle = helpers::build_image_bundle(image_options);

        let vendor_pubkey_digest = gen
            .vendor_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        let owner_pubkey_digest = gen
            .owner_pubkey_digest(&image_bundle.manifest.preamble)
            .unwrap();

        const FW_SVN: u32 = 2;

        let fuses = Fuses {
            anti_rollback_disable: false,
            fw_svn: [1, 0, 0, 0],
            vendor_pk_hash: vendor_pubkey_digest,
            owner_pk_hash: owner_pubkey_digest,
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_options = ImageOptions {
            vendor_config: VENDOR_CONFIG_KEY_1,
            fw_svn: FW_SVN,
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        let pcr_entry_arr = hw.mailbox_execute(0x1000_0000, &[]).unwrap().unwrap();

        // Fetch and validate PCR values against the log.

        let pcrs = hw.mailbox_execute(0x1000_0006, &[]).unwrap().unwrap();
        assert_eq!(pcrs.len(), PCR_COUNT * 48);

        let mut pcr0_from_hw: [u8; 48] = pcrs[0..48].try_into().unwrap();
        let mut pcr1_from_hw: [u8; 48] = pcrs[48..96].try_into().unwrap();

        helpers::change_dword_endianess(&mut pcr0_from_hw);
        helpers::change_dword_endianess(&mut pcr1_from_hw);

        let pcr0_from_log = hash_pcr_log_entries(&[0; 48], &pcr_entry_arr, PcrId::PcrId0);
        let pcr1_from_log = hash_pcr_log_entries(&[0; 48], &pcr_entry_arr, PcrId::PcrId1);

        assert_eq!(pcr0_from_log, pcr0_from_hw);
        assert_eq!(pcr1_from_log, pcr1_from_hw);

        // Ensure all other PCRs, except PCR0, PCR1 and PCR31, are empty.
        for i in 2..(PCR_COUNT - 1) {
            let offset = i * 48;
            assert_eq!(pcrs[offset..offset + 48], [0; 48]);
        }

        // Trigger an update reset.
        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();
        hw.step_until_boot_status(UpdateResetComplete.into(), true);

        let pcr_entry_arr = hw.mailbox_execute(0x1000_0000, &[]).unwrap().unwrap();

        // Fetch and validate PCR values against the log. PCR0 should represent the
        // latest boot, while PCR1 should represent the whole journey.

        let pcrs_after_reset = hw.mailbox_execute(0x1000_0006, &[]).unwrap().unwrap();
        assert_eq!(pcrs_after_reset.len(), PCR_COUNT * 48);

        let mut new_pcr0_from_hw: [u8; 48] = pcrs_after_reset[0..48].try_into().unwrap();
        let mut new_pcr1_from_hw: [u8; 48] = pcrs_after_reset[48..96].try_into().unwrap();

        helpers::change_dword_endianess(&mut new_pcr0_from_hw);
        helpers::change_dword_endianess(&mut new_pcr1_from_hw);

        let new_pcr0_from_log = hash_pcr_log_entries(&[0; 48], &pcr_entry_arr, PcrId::PcrId0);
        let new_pcr1_from_log = hash_pcr_log_entries(&pcr1_from_log, &pcr_entry_arr, PcrId::PcrId1);

        assert_eq!(new_pcr0_from_log, new_pcr0_from_hw);
        assert_eq!(new_pcr1_from_log, new_pcr1_from_hw);

        // Also ensure PCR locks are configured correctly.
        let reset_checks = hw.mailbox_execute(0x1000_0007, &[]).unwrap().unwrap();
        assert_eq!(reset_checks, [0; 4]);

        let pcrs_after_clear = hw.mailbox_execute(0x1000_0006, &[]).unwrap().unwrap();
        assert_eq!(pcrs_after_clear, pcrs_after_reset);
    }
}

#[test]
#[allow(deprecated)]
fn test_fuse_log() {
    const FW_SVN: u32 = 4;
    const FW_FUSE_SVN: u32 = 3;

    let fuses = Fuses {
        anti_rollback_disable: true,
        fw_svn: [0x7, 0, 0, 0], // Value of FW_FUSE_SVN
        fuse_pqc_key_type: FwVerificationPqcKeyType::LMS as u32,
        ..Default::default()
    };

    let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
    let mut hw = caliptra_hw_model::new(
        InitParams {
            rom: &rom,
            security_state: SecurityState::from(fuses.life_cycle as u32),
            ..Default::default()
        },
        BootParams {
            fuses,
            ..Default::default()
        },
    )
    .unwrap();

    let image_options = ImageOptions {
        vendor_config: VENDOR_CONFIG_KEY_1,
        owner_config: Some(OWNER_CONFIG),
        fmc_version: 0,
        app_version: 0,
        pqc_key_type: FwVerificationPqcKeyType::LMS,
        fw_svn: FW_SVN,
    };

    let image_bundle =
        caliptra_builder::build_and_sign_image(&TEST_FMC_WITH_UART, &APP_WITH_UART, image_options)
            .unwrap();

    hw.upload_firmware(&image_bundle.to_bytes().unwrap())
        .unwrap();

    hw.step_until_boot_status(u32::from(ColdResetComplete), true);

    let fuse_entry_arr = hw.mailbox_execute(0x1000_0002, &[]).unwrap().unwrap();

    let mut fuse_log_entry_offset = 0;

    // Check entry for VendorPubKeyIndex.
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();

    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::VendorEccPubKeyIndex as u32
    );

    assert_eq!(fuse_log_entry.log_data[0], VENDOR_CONFIG_KEY_1.ecc_key_idx);

    // Validate that the ID is VendorPubKeyRevocation
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::VendorEccPubKeyRevocation as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], 0,);

    // Validate the ColdBootFwSvn
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::ColdBootFwSvn as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], FW_SVN);

    // Validate the ManifestReserved0
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::ManifestReserved0 as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], 0);

    // Validate the _DeprecatedFuseFmcSvn
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::_DeprecatedFuseFmcSvn as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], FW_FUSE_SVN);

    // Validate the ManifestFwSvn
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::ManifestFwSvn as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], FW_SVN);

    // Validate the ManifestReserved1
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::ManifestReserved1 as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], 0);

    // Validate the FuseFwSvn
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(fuse_log_entry.entry_id, FuseLogEntryId::FuseFwSvn as u32);
    assert_eq!(fuse_log_entry.log_data[0], FW_FUSE_SVN);

    // Validate the VendorPqcPubKeyIndex
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::VendorPqcPubKeyIndex as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], VENDOR_CONFIG_KEY_1.pqc_key_idx);

    // Validate that the ID is VendorPubKeyRevocation
    fuse_log_entry_offset += core::mem::size_of::<FuseLogEntry>();
    let (fuse_log_entry, _) =
        FuseLogEntry::ref_from_prefix(fuse_entry_arr[fuse_log_entry_offset..].as_bytes()).unwrap();
    assert_eq!(
        fuse_log_entry.entry_id,
        FuseLogEntryId::VendorPqcPubKeyRevocation as u32
    );
    assert_eq!(fuse_log_entry.log_data[0], 0,);
}

#[test]
fn test_fht_info() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fuses = Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };

        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_WITH_UART,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();
        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        let data = hw.mailbox_execute(0x1000_0003, &[]).unwrap().unwrap();
        let fht = FirmwareHandoffTable::try_ref_from_bytes(data.as_bytes()).unwrap();
        assert_eq!(fht.ecc_ldevid_tbs_size, 552);
        assert_eq!(fht.ecc_fmcalias_tbs_size, 753);
    }
}

#[test]
fn test_check_rom_cold_boot_status_reg() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fuses = Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_WITH_UART,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        let data_vault = hw.mailbox_execute(0x1000_0005, &[]).unwrap().unwrap();
        let (data_vault, _) = DataVault::ref_from_prefix(data_vault.as_bytes()).unwrap();

        assert_eq!(
            data_vault.rom_cold_boot_status(),
            u32::from(ColdResetComplete)
        );
    }
}

#[test]
fn test_upload_single_measurement() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fuses = Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        // Upload measurement.
        let measurement = StashMeasurementReq {
            measurement: [0xdeadbeef_u32; 12].as_bytes().try_into().unwrap(),
            hdr: MailboxReqHeader { chksum: 0 },
            metadata: [0xAB; 4],
            context: [0xCD; 48],
            svn: 0xEF01,
        };

        // Calc and update checksum
        let checksum = caliptra_common::checksum::calc_checksum(
            u32::from(CommandId::STASH_MEASUREMENT),
            &measurement.as_bytes()[4..],
        );
        let measurement = StashMeasurementReq {
            hdr: MailboxReqHeader { chksum: checksum },
            ..measurement
        };

        hw.upload_measurement(measurement.as_bytes()).unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        // Check if the measurement was present in the measurement log.
        let measurement_log = hw.mailbox_execute(0x1000_000A, &[]).unwrap().unwrap();

        assert_eq!(measurement_log.len(), MEASUREMENT_ENTRY_SIZE);
        check_measurement_log_entry(&measurement_log, 0, &measurement);

        // Get PCR31
        let pcr31 = hw.mailbox_execute(0x1000_0009, &[]).unwrap().unwrap();

        // Check that the measurement was extended to PCR31.
        let expected_pcr = hash_measurement_log_entries(&measurement_log);
        assert_eq!(pcr31.as_bytes(), expected_pcr);

        let data = hw.mailbox_execute(0x1000_0003, &[]).unwrap().unwrap();
        let (fht, _) = FirmwareHandoffTable::try_ref_from_prefix(data.as_bytes()).unwrap();
        assert_eq!(fht.meas_log_index, 1);
    }
}

#[test]
fn test_upload_measurement_limit() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fuses = Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        let mut measurement = StashMeasurementReq {
            measurement: [0xdeadbeef_u32; 12].as_bytes().try_into().unwrap(),
            hdr: MailboxReqHeader { chksum: 0 },
            metadata: [0u8; 4],
            context: [0u8; 48],
            svn: 0,
        };

        // Upload 8 measurements.
        for idx in 0..8 {
            measurement.measurement[0] = idx;
            measurement.context[1] = idx;
            measurement.svn = idx as u32;

            // Calc and update checksum
            let checksum = caliptra_common::checksum::calc_checksum(
                u32::from(CommandId::STASH_MEASUREMENT),
                &measurement.as_bytes()[4..],
            );
            let measurement = StashMeasurementReq {
                hdr: MailboxReqHeader { chksum: checksum },
                ..measurement
            };

            hw.upload_measurement(measurement.as_bytes()).unwrap();
        }

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        // Check the measurement log.
        let measurement_log = hw.mailbox_execute(0x1000_000A, &[]).unwrap().unwrap();
        assert_eq!(
            measurement_log.len(),
            MEASUREMENT_ENTRY_SIZE * MEASUREMENT_MAX_COUNT
        );
        for idx in 0..8 {
            measurement.measurement[0] = idx;
            measurement.context[1] = idx;
            measurement.svn = idx as u32;
            check_measurement_log_entry(&measurement_log, idx as usize, &measurement);
        }

        // Get PCR31
        let pcr31 = hw.mailbox_execute(0x1000_0009, &[]).unwrap().unwrap();

        // Check that the measurement was extended to PCR31.
        let expected_pcr = hash_measurement_log_entries(&measurement_log);
        assert_eq!(pcr31.as_bytes(), expected_pcr);

        let data = hw.mailbox_execute(0x1000_0003, &[]).unwrap().unwrap();
        let fht = FirmwareHandoffTable::try_ref_from_bytes(data.as_bytes()).unwrap();
        assert_eq!(fht.meas_log_index, MEASUREMENT_MAX_COUNT as u32);
    }
}

#[test]
fn test_upload_measurement_limit_plus_one() {
    let fuses = Fuses::default();
    let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
    let mut hw = caliptra_hw_model::new(
        InitParams {
            rom: &rom,
            security_state: SecurityState::from(fuses.life_cycle as u32),
            ..Default::default()
        },
        BootParams {
            fuses,
            ..Default::default()
        },
    )
    .unwrap();

    let mut measurement = StashMeasurementReq {
        measurement: [0xdeadbeef_u32; 12].as_bytes().try_into().unwrap(),
        hdr: MailboxReqHeader { chksum: 0 },
        metadata: [0u8; 4],
        context: [0u8; 48],
        svn: 0,
    };

    // Upload 8 measurements.
    for idx in 0..8 {
        measurement.measurement[0] = idx;
        measurement.context[1] = idx;
        measurement.svn = idx as u32;

        // Calc and update checksum
        let checksum = caliptra_common::checksum::calc_checksum(
            u32::from(CommandId::STASH_MEASUREMENT),
            &measurement.as_bytes()[4..],
        );
        let measurement = StashMeasurementReq {
            hdr: MailboxReqHeader { chksum: checksum },
            ..measurement
        };

        hw.upload_measurement(measurement.as_bytes()).unwrap();
    }

    // Upload a 9th measurement, which should fail and raise a fatal error.
    let result = hw.upload_measurement(measurement.as_bytes());
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ModelError::MailboxCmdFailed(_)
    ));

    // Wait for error
    while hw.soc_ifc().cptra_fw_error_fatal().read() == 0 {
        hw.step();
    }

    assert_eq!(
        hw.soc_ifc().cptra_fw_error_fatal().read(),
        u32::from(CaliptraError::FW_PROC_MAILBOX_STASH_MEASUREMENT_MAX_LIMIT)
    );
}

#[test]
fn test_upload_no_measurement() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fuses = Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let rom = caliptra_builder::build_firmware_rom(firmware::rom_from_env()).unwrap();
        let mut hw = caliptra_hw_model::new(
            InitParams {
                rom: &rom,
                security_state: SecurityState::from(fuses.life_cycle as u32),
                ..Default::default()
            },
            BootParams {
                fuses,
                ..Default::default()
            },
        )
        .unwrap();

        let image_bundle = caliptra_builder::build_and_sign_image(
            &TEST_FMC_INTERACTIVE,
            &APP_WITH_UART,
            image_options,
        )
        .unwrap();

        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(u32::from(ColdResetComplete), true);

        // Check whether the fake measurement was extended to PCR31.
        let pcr31 = hw.mailbox_execute(0x1000_0009, &[]).unwrap().unwrap();
        assert_eq!(pcr31.as_bytes(), [0u8; 48]);

        // Check whether the fake measurement is in the measurement log.
        let measurement_log = hw.mailbox_execute(0x1000_000A, &[]).unwrap().unwrap();
        assert_eq!(measurement_log.len(), 0);

        let data = hw.mailbox_execute(0x1000_0003, &[]).unwrap().unwrap();
        let fht = FirmwareHandoffTable::try_ref_from_bytes(data.as_bytes()).unwrap();
        assert_eq!(fht.meas_log_index, 0);
    }
}
