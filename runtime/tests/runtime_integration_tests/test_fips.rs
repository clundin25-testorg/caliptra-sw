// Licensed under the Apache-2.0 license.

use crate::common::{assert_error, run_rt_test, RuntimeTestArgs};
use caliptra_api::SocManager;
use caliptra_builder::{version, ImageOptions};
use caliptra_common::mailbox_api::{
    CommandId, FipsVersionResp, MailboxReqHeader, MailboxRespHeader,
};
use caliptra_hw_model::HwModel;
use caliptra_image_types::FwVerificationPqcKeyType;
use caliptra_runtime::FipsVersionCmd;
use zerocopy::{FromBytes, IntoBytes};

const HW_REV_ID: u32 = 0x02;

#[test]
fn test_fips_version() {
    let args = RuntimeTestArgs {
        test_image_options: Some(ImageOptions {
            fmc_version: version::get_fmc_version(),
            app_version: version::get_runtime_version(),
            pqc_key_type: FwVerificationPqcKeyType::LMS,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut model = run_rt_test(args);

    model.step_until(|m| m.soc_mbox().status().read().mbox_fsm_ps().mbox_idle());

    // VERSION
    let payload = MailboxReqHeader {
        chksum: caliptra_common::checksum::calc_checksum(u32::from(CommandId::VERSION), &[]),
    };

    let fips_version_resp = model
        .mailbox_execute(u32::from(CommandId::VERSION), payload.as_bytes())
        .unwrap()
        .unwrap();

    // Check command size
    let fips_version_bytes: &[u8] = fips_version_resp.as_bytes();

    // Check values against expected.
    let fips_version = FipsVersionResp::read_from_bytes(fips_version_bytes).unwrap();
    assert!(caliptra_common::checksum::verify_checksum(
        fips_version.hdr.chksum,
        0x0,
        &fips_version.as_bytes()[core::mem::size_of_val(&fips_version.hdr.chksum)..],
    ));
    assert_eq!(
        fips_version.hdr.fips_status,
        MailboxRespHeader::FIPS_STATUS_APPROVED
    );
    assert_eq!(fips_version.mode, FipsVersionCmd::MODE);
    // fw_rev[0] is FMC version at 31:16 and ROM version at 15:0
    // Ignore ROM version since this test is for runtime
    let fw_version_0_expected = (version::get_fmc_version() as u32) << 16;
    assert_eq!(
        [
            fips_version.fips_rev[0],
            fips_version.fips_rev[1] & 0xFFFF0000, // Mask out the ROM version
            fips_version.fips_rev[2],
        ],
        [
            HW_REV_ID,
            fw_version_0_expected,
            version::get_runtime_version()
        ]
    );
    let name = &fips_version.name[..];
    assert_eq!(name, FipsVersionCmd::NAME.as_bytes());
}

#[test]
fn test_fips_shutdown() {
    let mut model = run_rt_test(RuntimeTestArgs::default());

    model.step_until(|m| m.soc_mbox().status().read().mbox_fsm_ps().mbox_idle());

    // SHUTDOWN
    let payload = MailboxReqHeader {
        chksum: caliptra_common::checksum::calc_checksum(u32::from(CommandId::SHUTDOWN), &[]),
    };

    let resp = model
        .mailbox_execute(u32::from(CommandId::SHUTDOWN), payload.as_bytes())
        .unwrap()
        .unwrap();

    let resp = MailboxRespHeader::read_from_bytes(resp.as_slice()).unwrap();
    // Verify checksum and FIPS status
    assert!(caliptra_common::checksum::verify_checksum(
        resp.chksum,
        0x0,
        &resp.as_bytes()[core::mem::size_of_val(&resp.chksum)..],
    ));
    assert_eq!(resp.fips_status, MailboxRespHeader::FIPS_STATUS_APPROVED);

    // Check we are rejecting additional commands with the shutdown error code.
    let payload = MailboxReqHeader {
        chksum: caliptra_common::checksum::calc_checksum(u32::from(CommandId::VERSION), &[]),
    };
    let resp = model
        .mailbox_execute(u32::from(CommandId::VERSION), payload.as_bytes())
        .unwrap_err();
    assert_error(
        &mut model,
        caliptra_drivers::CaliptraError::RUNTIME_SHUTDOWN,
        resp,
    );
}
