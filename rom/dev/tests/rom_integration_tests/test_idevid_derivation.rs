// Licensed under the Apache-2.0 license

use caliptra_api::SocManager;
use caliptra_builder::{firmware, ImageOptions};
use caliptra_common::mailbox_api::{CommandId, GetLdevCertResp, MailboxReqHeader};
use caliptra_drivers::{IdevidCertAttr, InitDevIdCsrEnvelope, MfgFlags, X509KeyIdAlgo};
use caliptra_hw_model::{DefaultHwModel, Fuses, HwModel};
use caliptra_image_types::{FwVerificationPqcKeyType, ImageBundle};
use openssl::pkey::{PKey, Public};
use openssl::x509::X509;
use openssl::{rand::rand_bytes, x509::X509Req};
use zerocopy::IntoBytes;

use crate::helpers;

const RT_READY_FOR_COMMANDS: u32 = 0x600;

fn generate_csr_envelop(
    hw: &mut DefaultHwModel,
    image_bundle: &ImageBundle,
) -> InitDevIdCsrEnvelope {
    // Set gen_idev_id_csr to generate CSR.
    let flags = MfgFlags::GENERATE_IDEVID_CSR;
    hw.soc_ifc()
        .cptra_dbg_manuf_service_reg()
        .write(|_| flags.bits());

    // Download the CSR Envelope from the mailbox.
    let csr_envelop = helpers::get_csr_envelop(hw).unwrap();

    // Wait for uploading firmware.
    hw.step_until(|m| {
        m.soc_ifc()
            .cptra_flow_status()
            .read()
            .ready_for_mb_processing()
    });
    hw.upload_firmware(&image_bundle.to_bytes().unwrap())
        .unwrap();

    hw.step_until_boot_status(RT_READY_FOR_COMMANDS, true);

    let output = hw.output().take(usize::MAX);
    if firmware::rom_from_env() == &firmware::ROM_WITH_UART {
        let csr_str = helpers::get_data("[idev] ECC CSR = ", &output);
        let uploaded = hex::decode(csr_str).unwrap();
        assert_eq!(
            uploaded,
            &csr_envelop.ecc_csr.csr[..csr_envelop.ecc_csr.csr_len as usize]
        );
    }
    csr_envelop
}

#[test]
fn test_generate_csr_envelop() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let fuses = Fuses {
            fuse_pqc_key_type: *pqc_key_type as u32,
            ..Default::default()
        };
        let (mut hw, image_bundle) = helpers::build_hw_model_and_image_bundle(fuses, image_options);
        generate_csr_envelop(&mut hw, &image_bundle);
    }
}

#[test]
fn test_ecc_idev_subj_key_id_algo() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        for algo in 0..(X509KeyIdAlgo::Fuse as u32 + 1) {
            let mut fuses = Fuses {
                fuse_pqc_key_type: *pqc_key_type as u32,
                ..Default::default()
            };
            fuses.idevid_cert_attr[IdevidCertAttr::Flags as usize] = algo;

            let (mut hw, image_bundle) =
                helpers::build_hw_model_and_image_bundle(fuses, image_options.clone());
            hw.upload_firmware(&image_bundle.to_bytes().unwrap())
                .unwrap();

            hw.step_until_boot_status(RT_READY_FOR_COMMANDS, true);
        }
    }
}

#[test]
fn test_mldsa_idev_subj_key_id_algo() {
    for algo in 0..(X509KeyIdAlgo::Fuse as u32 + 1) {
        let mut fuses = Fuses::default();
        fuses.idevid_cert_attr[IdevidCertAttr::Flags as usize] = algo;

        let image_options = ImageOptions {
            pqc_key_type: FwVerificationPqcKeyType::MLDSA,
            ..Default::default()
        };

        let (mut hw, image_bundle) = helpers::build_hw_model_and_image_bundle(fuses, image_options);
        hw.upload_firmware(&image_bundle.to_bytes().unwrap())
            .unwrap();

        hw.step_until_boot_status(RT_READY_FOR_COMMANDS, true);
    }
}

fn fuses_with_random_uds() -> Fuses {
    const UDS_LEN: usize = core::mem::size_of::<u32>() * 16;
    let mut uds_bytes = [0; UDS_LEN];
    rand_bytes(&mut uds_bytes).unwrap();
    let mut uds_seed = [0u32; 16];

    for (word, bytes) in uds_seed.iter_mut().zip(uds_bytes.chunks_exact(4)) {
        *word = u32::from_be_bytes(bytes.try_into().unwrap());
    }
    Fuses {
        uds_seed,
        ..Default::default()
    }
}

#[test]
fn test_generate_csr_envelop_stress() {
    for pqc_key_type in helpers::PQC_KEY_TYPE.iter() {
        let image_options = ImageOptions {
            pqc_key_type: *pqc_key_type,
            ..Default::default()
        };
        let num_tests = if cfg!(feature = "slow_tests") {
            1000
        } else {
            1
        };

        for _ in 0..num_tests {
            let mut fuses = fuses_with_random_uds();
            fuses.fuse_pqc_key_type = *pqc_key_type as u32;
            let (mut hw, image_bundle) =
                helpers::build_hw_model_and_image_bundle(fuses.clone(), image_options.clone());

            let csr_envelop = generate_csr_envelop(&mut hw, &image_bundle);

            // Ensure ECC CSR is valid X.509
            let req =
                X509Req::from_der(&csr_envelop.ecc_csr.csr[..csr_envelop.ecc_csr.csr_len as usize])
                    .unwrap_or_else(|_| {
                        panic!(
                            "Failed to create a valid X509 cert with UDS seed {:?}",
                            fuses.uds_seed
                        )
                    });
            let idevid_pubkey = req.public_key().unwrap();
            assert!(
                req.verify(&idevid_pubkey).unwrap(),
                "Invalid public key. Unable to verify CSR with UDS seed {:?}",
                fuses.uds_seed
            );

            let ldev_cert = verify_key(
                &mut hw,
                u32::from(CommandId::GET_LDEV_CERT),
                &idevid_pubkey,
                &fuses.uds_seed,
            );
            let fmc_cert = verify_key(
                &mut hw,
                u32::from(CommandId::GET_FMC_ALIAS_CERT),
                &ldev_cert.public_key().unwrap(),
                &fuses.uds_seed,
            );
            let _rt_cert = verify_key(
                &mut hw,
                u32::from(CommandId::GET_RT_ALIAS_CERT),
                &fmc_cert.public_key().unwrap(),
                &fuses.uds_seed,
            );
        }
    }
}

fn verify_key(
    hw: &mut DefaultHwModel,
    cmd_id: u32,
    pubkey: &PKey<Public>,
    test_uds: &[u32; 16],
) -> X509 {
    let payload = MailboxReqHeader {
        chksum: caliptra_common::checksum::calc_checksum(cmd_id, &[]),
    };

    // Execute the command
    let resp = hw
        .mailbox_execute(cmd_id, payload.as_bytes())
        .unwrap()
        .unwrap();

    assert!(resp.len() <= std::mem::size_of::<GetLdevCertResp>());
    let mut cert_resp = GetLdevCertResp::default();
    cert_resp.as_mut_bytes()[..resp.len()].copy_from_slice(&resp);

    // Extract the certificate from the response
    let cert_der = &cert_resp.data[..(cert_resp.data_size as usize)];
    let cert = openssl::x509::X509::from_der(cert_der).unwrap();

    assert!(
        cert.verify(pubkey).unwrap(),
        "{:?} cert failed to validate with {:?} pubkey with UDS: {test_uds:?}",
        cert.subject_name(),
        cert.issuer_name(),
    );

    cert
}
