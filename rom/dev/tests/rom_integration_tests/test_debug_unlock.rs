// Licensed under the Apache-2.0 license

use std::mem::size_of;

use caliptra_api::mailbox::{
    CommandId, MailboxReqHeader, ManufDebugUnlockTokenReq, ProductionAuthDebugUnlockChallenge,
    ProductionAuthDebugUnlockReq, ProductionAuthDebugUnlockToken,
};
use caliptra_api::SocManager;
use caliptra_builder::firmware::ROM_WITH_UART;
use caliptra_error::CaliptraError;
use caliptra_hw_model::{
    DbgManufServiceRegReq, DeviceLifecycle, HwModel, ModelError, SecurityState,
};
use fips204::traits::{SerDes, Signer};
use p384::ecdsa::VerifyingKey;
use rand::{rngs::StdRng, SeedableRng};
use sha2::Digest;
use zerocopy::{FromBytes, IntoBytes};

#[test]
fn test_dbg_unlock_manuf_passive_mode() {
    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Manufacturing);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_manuf_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            debug_intent: true,
            active_mode: false,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let token = ManufDebugUnlockTokenReq {
        token: caliptra_hw_model_types::DEFAULT_MANUF_DEBUG_UNLOCK_TOKEN
            .as_bytes()
            .try_into()
            .unwrap(),
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ManufDebugUnlockTokenReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };
    assert_eq!(
        hw.mailbox_execute(
            CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN.into(),
            token.as_bytes(),
        ),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_REQ_IN_PASSIVE_MODE.into()
        ))
    );
}

#[test]
fn test_dbg_unlock_manuf() {
    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Manufacturing);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_manuf_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let token = ManufDebugUnlockTokenReq {
        token: caliptra_hw_model_types::DEFAULT_MANUF_DEBUG_UNLOCK_TOKEN
            .as_bytes()
            .try_into()
            .unwrap(),
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ManufDebugUnlockTokenReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };
    hw.mailbox_execute(
        CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN.into(),
        token.as_bytes(),
    )
    .unwrap();

    hw.step_until(|m| {
        let resp = m.soc_ifc().ss_dbg_manuf_service_reg_rsp().read();
        resp.manuf_dbg_unlock_success()
    });
}

#[test]
fn test_dbg_unlock_manuf_wrong_cmd() {
    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Manufacturing);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_manuf_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let token = ManufDebugUnlockTokenReq {
        token: caliptra_hw_model_types::DEFAULT_MANUF_DEBUG_UNLOCK_TOKEN
            .as_bytes()
            .try_into()
            .unwrap(),
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ManufDebugUnlockTokenReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };
    assert_eq!(
        hw.mailbox_execute(0, token.as_bytes(),),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_MANUF_INVALID_MBOX_CMD.into()
        ))
    );
}

#[test]
fn test_dbg_unlock_manuf_invalid_token() {
    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Manufacturing);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_manuf_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    // Defaults to 0 token
    let token = ManufDebugUnlockTokenReq {
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ManufDebugUnlockTokenReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };
    assert_eq!(
        hw.mailbox_execute(
            CommandId::MANUF_DEBUG_UNLOCK_REQ_TOKEN.into(),
            token.as_bytes()
        ),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_MANUF_INVALID_TOKEN.into()
        ))
    );

    hw.step_until(|m| {
        let resp = m.soc_ifc().ss_dbg_manuf_service_reg_rsp().read();
        resp.manuf_dbg_unlock_fail()
    });
}

#[test]
fn test_dbg_unlock_prod() {
    let signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let verifying_ecc_key = VerifyingKey::from(&signing_ecc_key);
    let ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (verifying_mldsa_key, signing_mldsa_key) = fips204::ml_dsa_87::try_keygen().unwrap();
    let mldsa_pub_key_bytes = verifying_mldsa_key.into_bytes();
    let mldsa_pub_key_reversed = {
        let mut key = mldsa_pub_key_bytes;
        key.reverse();
        key
    };

    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Production);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_prod_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            prod_dbg_unlock_keypairs: vec![(&ecc_pub_key_bytes, &mldsa_pub_key_reversed)],
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let unlock_category = {
        let unlock: u32 = 0;
        let unlock_bytes = unlock.to_le_bytes();
        unlock_bytes[..3].try_into().unwrap()
    };

    // [TODO][CAP2] With wrong len mbox err 0 gets returned which is not right
    let request = ProductionAuthDebugUnlockReq {
        length: {
            let req_len = size_of::<ProductionAuthDebugUnlockReq>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unlock_category,
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ),
        &request.as_bytes()[4..],
    );
    let request = ProductionAuthDebugUnlockReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..request
    };
    let resp = hw
        .mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ.into(),
            request.as_bytes(),
        )
        .unwrap()
        .unwrap();

    let challenge = ProductionAuthDebugUnlockChallenge::read_from_bytes(resp.as_slice()).unwrap();

    let mut sha384 = sha2::Sha384::new();
    sha384.update(challenge.challenge);
    sha384.update(challenge.unique_device_identifier);
    sha384.update(unlock_category);
    let sha384_digest = sha384.finalize();
    let (ecc_signature, _id) = signing_ecc_key
        .sign_prehash_recoverable(sha384_digest.as_slice())
        .unwrap();
    let ecc_signature = ecc_signature.to_bytes();
    let ecc_signature = ecc_signature.as_slice();

    let mut sha512 = sha2::Sha512::new();
    sha512.update(challenge.challenge);
    sha512.update(challenge.unique_device_identifier);
    sha512.update(unlock_category);
    let mut sha512_digest = sha512.finalize();
    let msg = {
        let msg: &mut [u8] = sha512_digest.as_mut_slice();
        msg.reverse();
        msg
    };

    let mldsa_signature = signing_mldsa_key
        .try_sign_with_seed(&[0; 32], msg, &[])
        .unwrap();

    let token = ProductionAuthDebugUnlockToken {
        length: {
            let req_len =
                size_of::<ProductionAuthDebugUnlockToken>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unique_device_identifier: challenge.unique_device_identifier,
        unlock_category,
        challenge: challenge.challenge,
        ecc_public_key: ecc_pub_key_bytes,
        mldsa_public_key: mldsa_pub_key_reversed,
        ecc_signature: ecc_signature.try_into().unwrap(),
        mldsa_signature: {
            let mut sig = [0; 4628];
            sig[..4627].copy_from_slice(&mldsa_signature);
            sig.reverse();
            sig
        },
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ProductionAuthDebugUnlockToken {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };

    let _resp = hw
        .mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN.into(),
            token.as_bytes(),
        )
        .unwrap();

    hw.step_until(|m| {
        let resp = m.soc_ifc().ss_dbg_manuf_service_reg_rsp().read();
        resp.prod_dbg_unlock_success()
    });
}

#[test]
fn test_dbg_unlock_prod_invalid_length() {
    let signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let verifying_ecc_key = VerifyingKey::from(&signing_ecc_key);
    let ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (verifying_mldsa_key, _signing_mldsa_key) = fips204::ml_dsa_87::try_keygen().unwrap();
    let mldsa_pub_key_bytes = verifying_mldsa_key.into_bytes();

    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Production);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_prod_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            prod_dbg_unlock_keypairs: vec![(&ecc_pub_key_bytes, &mldsa_pub_key_bytes)],
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let unlock_category = {
        let unlock: u32 = 0;
        let unlock_bytes = unlock.to_le_bytes();
        unlock_bytes[..3].try_into().unwrap()
    };

    let request = ProductionAuthDebugUnlockReq {
        length: {
            // Set an incorrect length
            let wrong_len = 123u32;
            let wrong_len_bytes = wrong_len.to_le_bytes();
            wrong_len_bytes[..3].try_into().unwrap()
        },
        unlock_category,
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ),
        &request.as_bytes()[4..],
    );
    let request = ProductionAuthDebugUnlockReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..request
    };
    assert_eq!(
        hw.mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ.into(),
            request.as_bytes(),
        ),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_PROD_INVALID_REQ.into()
        ))
    );
}

#[test]
fn test_dbg_unlock_prod_invalid_token_challenage() {
    let signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let verifying_ecc_key = VerifyingKey::from(&signing_ecc_key);
    let ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (verifying_mldsa_key, _signing_mldsa_key) = fips204::ml_dsa_87::try_keygen().unwrap();
    let mldsa_pub_key_bytes = verifying_mldsa_key.into_bytes();

    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Production);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_prod_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            prod_dbg_unlock_keypairs: vec![(&ecc_pub_key_bytes, &mldsa_pub_key_bytes)],
            active_mode: true,
            debug_intent: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let unlock_category = {
        let unlock: u32 = 0;
        let unlock_bytes = unlock.to_le_bytes();
        unlock_bytes[..3].try_into().unwrap()
    };

    let request = ProductionAuthDebugUnlockReq {
        length: {
            let req_len = size_of::<ProductionAuthDebugUnlockReq>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unlock_category,
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ),
        &request.as_bytes()[4..],
    );
    let request = ProductionAuthDebugUnlockReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..request
    };
    let resp = hw
        .mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ.into(),
            request.as_bytes(),
        )
        .unwrap()
        .unwrap();

    let challenge = ProductionAuthDebugUnlockChallenge::read_from_bytes(resp.as_slice()).unwrap();

    // Create an invalid token by using a different challenge than what was received
    let invalid_challenge = [0u8; 48];

    let token = ProductionAuthDebugUnlockToken {
        length: {
            let req_len =
                size_of::<ProductionAuthDebugUnlockToken>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unique_device_identifier: challenge.unique_device_identifier,
        unlock_category,
        challenge: invalid_challenge, // Use invalid challenge
        ecc_public_key: ecc_pub_key_bytes,
        mldsa_public_key: mldsa_pub_key_bytes,
        ecc_signature: [0u8; 96],     // Invalid signature
        mldsa_signature: [0u8; 4628], // Invalid signature
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ProductionAuthDebugUnlockToken {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };

    assert_eq!(
        hw.mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN.into(),
            token.as_bytes(),
        ),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_PROD_INVALID_TOKEN_CHALLENGE.into()
        ))
    );

    hw.step_until(|m| {
        let resp = m.soc_ifc().ss_dbg_manuf_service_reg_rsp().read();
        resp.prod_dbg_unlock_fail()
    });
}

#[test]
fn test_dbg_unlock_prod_invalid_signature() {
    let signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let verifying_ecc_key = VerifyingKey::from(&signing_ecc_key);
    let ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (verifying_mldsa_key, signing_mldsa_key) = fips204::ml_dsa_87::try_keygen().unwrap();
    let mldsa_pub_key_bytes = verifying_mldsa_key.into_bytes();
    let mldsa_pub_key_reversed = {
        let mut key = mldsa_pub_key_bytes;
        key.reverse();
        key
    };

    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Production);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_prod_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            prod_dbg_unlock_keypairs: vec![(&ecc_pub_key_bytes, &mldsa_pub_key_reversed)],
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let unlock_category = {
        let unlock: u32 = 0;
        let unlock_bytes = unlock.to_le_bytes();
        unlock_bytes[..3].try_into().unwrap()
    };

    // [TODO][CAP2] With wrong len mbox err 0 gets returned which is not right
    let request = ProductionAuthDebugUnlockReq {
        length: {
            let req_len = size_of::<ProductionAuthDebugUnlockReq>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unlock_category,
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ),
        &request.as_bytes()[4..],
    );
    let request = ProductionAuthDebugUnlockReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..request
    };
    let resp = hw
        .mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ.into(),
            request.as_bytes(),
        )
        .unwrap()
        .unwrap();

    let challenge = ProductionAuthDebugUnlockChallenge::read_from_bytes(resp.as_slice()).unwrap();

    let mut sha512 = sha2::Sha512::new();
    sha512.update(challenge.challenge);
    sha512.update(challenge.unique_device_identifier);
    sha512.update(unlock_category);
    let mut sha512_digest = sha512.finalize();
    let msg = {
        let msg: &mut [u8] = sha512_digest.as_mut_slice();
        msg.reverse();
        msg
    };

    let mldsa_signature = signing_mldsa_key
        .try_sign_with_seed(&[0; 32], msg, &[])
        .unwrap();

    let token = ProductionAuthDebugUnlockToken {
        length: {
            let req_len =
                size_of::<ProductionAuthDebugUnlockToken>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unique_device_identifier: challenge.unique_device_identifier,
        unlock_category,
        challenge: challenge.challenge,
        ecc_public_key: ecc_pub_key_bytes,
        mldsa_public_key: mldsa_pub_key_reversed,
        ecc_signature: [0xab; 96], // Invalid signature
        mldsa_signature: {
            let mut sig = [0; 4628];
            sig[..4627].copy_from_slice(&mldsa_signature);
            sig.reverse();
            sig
        },
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ProductionAuthDebugUnlockToken {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };

    assert_eq!(
        hw.mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN.into(),
            token.as_bytes(),
        ),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_PROD_INVALID_TOKEN_INVALID_SIGNATURE.into()
        ))
    );

    hw.step_until(|m| {
        let resp = m.soc_ifc().ss_dbg_manuf_service_reg_rsp().read();
        resp.prod_dbg_unlock_fail()
    });
}

#[test]
fn test_dbg_unlock_prod_wrong_public_keys() {
    let signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let verifying_ecc_key = VerifyingKey::from(&signing_ecc_key);
    let ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (verifying_mldsa_key, _signing_mldsa_key) = fips204::ml_dsa_87::try_keygen().unwrap();
    let mldsa_pub_key_bytes = verifying_mldsa_key.into_bytes();

    // Generate a different set of keys that aren't registered with the hardware
    let different_signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let different_verifying_ecc_key = VerifyingKey::from(&different_signing_ecc_key);
    let different_ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = different_verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (different_verifying_mldsa_key, _) = fips204::ml_dsa_87::try_keygen().unwrap();
    let different_mldsa_pub_key_bytes = different_verifying_mldsa_key.into_bytes();

    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Production);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_prod_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            prod_dbg_unlock_keypairs: vec![(&ecc_pub_key_bytes, &mldsa_pub_key_bytes)],
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let unlock_category = {
        let unlock: u32 = 0;
        let unlock_bytes = unlock.to_le_bytes();
        unlock_bytes[..3].try_into().unwrap()
    };

    let request = ProductionAuthDebugUnlockReq {
        length: {
            let req_len = size_of::<ProductionAuthDebugUnlockReq>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unlock_category,
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ),
        &request.as_bytes()[4..],
    );
    let request = ProductionAuthDebugUnlockReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..request
    };
    let resp = hw
        .mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ.into(),
            request.as_bytes(),
        )
        .unwrap()
        .unwrap();

    let challenge = ProductionAuthDebugUnlockChallenge::read_from_bytes(resp.as_slice()).unwrap();

    let token = ProductionAuthDebugUnlockToken {
        length: {
            let req_len =
                size_of::<ProductionAuthDebugUnlockToken>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unique_device_identifier: challenge.unique_device_identifier,
        unlock_category,
        challenge: challenge.challenge,
        // Use the different public keys that weren't registered with the hardware
        ecc_public_key: different_ecc_pub_key_bytes,
        mldsa_public_key: different_mldsa_pub_key_bytes,
        ecc_signature: [0u8; 96], // Signature doesn't matter since keys will fail first
        mldsa_signature: [0u8; 4628], // Signature doesn't matter since keys will fail first
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN),
        &token.as_bytes()[4..],
    );
    let token = ProductionAuthDebugUnlockToken {
        hdr: MailboxReqHeader { chksum: checksum },
        ..token
    };

    assert_eq!(
        hw.mailbox_execute(
            CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_TOKEN.into(),
            token.as_bytes(),
        ),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_PROD_INVALID_TOKEN_WRONG_PUBLIC_KEYS.into()
        ))
    );

    hw.step_until(|m| {
        let resp = m.soc_ifc().ss_dbg_manuf_service_reg_rsp().read();
        resp.prod_dbg_unlock_fail()
    });
}

#[test]
fn test_dbg_unlock_prod_wrong_cmd() {
    let signing_ecc_key = p384::ecdsa::SigningKey::random(&mut StdRng::from_entropy());
    let verifying_ecc_key = VerifyingKey::from(&signing_ecc_key);
    let ecc_pub_key_bytes = {
        let mut pk = [0; 96];
        let ecc_key = verifying_ecc_key.to_encoded_point(false);
        pk[..48].copy_from_slice(ecc_key.x().unwrap());
        pk[48..].copy_from_slice(ecc_key.y().unwrap());
        pk
    };

    let (verifying_mldsa_key, _signing_mldsa_key) = fips204::ml_dsa_87::try_keygen().unwrap();
    let mldsa_pub_key_bytes = verifying_mldsa_key.into_bytes();

    let security_state = *SecurityState::default()
        .set_debug_locked(true)
        .set_device_lifecycle(DeviceLifecycle::Production);

    let dbg_manuf_service = *DbgManufServiceRegReq::default().set_prod_dbg_unlock_req(true);

    let rom = caliptra_builder::build_firmware_rom(&ROM_WITH_UART).unwrap();

    let mut hw = caliptra_hw_model::new(
        caliptra_hw_model::InitParams {
            rom: &rom,
            security_state,
            dbg_manuf_service,
            prod_dbg_unlock_keypairs: vec![(&ecc_pub_key_bytes, &mldsa_pub_key_bytes)],
            debug_intent: true,
            active_mode: true,
            ..Default::default()
        },
        caliptra_hw_model::BootParams::default(),
    )
    .unwrap();

    let unlock_category = {
        let unlock: u32 = 0;
        let unlock_bytes = unlock.to_le_bytes();
        unlock_bytes[..3].try_into().unwrap()
    };

    let request = ProductionAuthDebugUnlockReq {
        length: {
            let req_len = size_of::<ProductionAuthDebugUnlockReq>() - size_of::<MailboxReqHeader>();
            let struct_len = (req_len / size_of::<u32>()) as u32;
            let struct_len_bytes = struct_len.to_le_bytes();
            struct_len_bytes[..3].try_into().unwrap()
        },
        unlock_category,
        ..Default::default()
    };
    let checksum = caliptra_common::checksum::calc_checksum(
        u32::from(CommandId::PRODUCTION_AUTH_DEBUG_UNLOCK_REQ),
        &request.as_bytes()[4..],
    );
    let request = ProductionAuthDebugUnlockReq {
        hdr: MailboxReqHeader { chksum: checksum },
        ..request
    };
    assert_eq!(
        hw.mailbox_execute(0, request.as_bytes()),
        Err(ModelError::MailboxCmdFailed(
            CaliptraError::ROM_SS_DBG_UNLOCK_PROD_INVALID_REQ_MBOX_CMD.into()
        ))
    );
}
