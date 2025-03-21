/*++

Licensed under the Apache-2.0 license.

File Name:

    error_reporter_tests.rs

Abstract:

    File contains test cases for HMAC-384 API

--*/

#![no_std]
#![no_main]

use caliptra_drivers::{report_boot_status, SocIfc};
use caliptra_registers::soc_ifc::SocIfcReg;

use caliptra_test_harness::test_suite;

fn retrieve_boot_status() -> u32 {
    let soc_ifc = unsafe { SocIfcReg::new() };
    soc_ifc.regs().cptra_boot_status().read()
}

fn test_report_boot_status() {
    let val: u32 = 0xbeef2;
    report_boot_status(val);
    assert_eq!(val, retrieve_boot_status());
}

fn test_report_idevid_csr_ready() {
    let soc_ifc = unsafe { SocIfcReg::new() };
    SocIfc::new(soc_ifc).flow_status_set_idevid_csr_ready();
    let soc_ifc = unsafe { SocIfcReg::new() };
    assert!(soc_ifc.regs().cptra_flow_status().read().idevid_csr_ready());
}

fn test_report_ready_for_firmware() {
    let soc_ifc = unsafe { SocIfcReg::new() };
    SocIfc::new(soc_ifc).flow_status_set_ready_for_mb_processing();
    let soc_ifc = unsafe { SocIfcReg::new() };
    assert!(soc_ifc
        .regs()
        .cptra_flow_status()
        .read()
        .ready_for_mb_processing());
}

test_suite! {
    test_report_boot_status,
    test_report_idevid_csr_ready,
    test_report_ready_for_firmware,
}
