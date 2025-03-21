// Licensed under the Apache-2.0 license.
//
// generated by caliptra_registers_generator with caliptra-rtl repo at fc2ec4574c8662d0d208c8cfd916ea5dcc312ef7
//
#![allow(clippy::erasing_op)]
#![allow(clippy::identity_op)]
/// A zero-sized type that represents ownership of this
/// peripheral, used to get access to a Register lock. Most
/// programs create one of these in unsafe code near the top of
/// main(), and pass it to the driver responsible for managing
/// all access to the hardware.
pub struct AesReg {
    _priv: (),
}
impl AesReg {
    pub const PTR: *mut u32 = 0x10011000 as *mut u32;
    /// # Safety
    ///
    /// Caller must ensure that all concurrent use of this
    /// peripheral in the firmware is done so in a compatible
    /// way. The simplest way to enforce this is to only call
    /// this function once.
    #[inline(always)]
    pub unsafe fn new() -> Self {
        Self { _priv: () }
    }
    /// Returns a register block that can be used to read
    /// registers from this peripheral, but cannot write.
    #[inline(always)]
    pub fn regs(&self) -> RegisterBlock<ureg::RealMmio> {
        RegisterBlock {
            ptr: Self::PTR,
            mmio: core::default::Default::default(),
        }
    }
    /// Return a register block that can be used to read and
    /// write this peripheral's registers.
    #[inline(always)]
    pub fn regs_mut(&mut self) -> RegisterBlock<ureg::RealMmioMut> {
        RegisterBlock {
            ptr: Self::PTR,
            mmio: core::default::Default::default(),
        }
    }
}
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct RegisterBlock<TMmio: ureg::Mmio + core::borrow::Borrow<TMmio>> {
    ptr: *mut u32,
    mmio: TMmio,
}
impl<TMmio: ureg::Mmio + core::default::Default> RegisterBlock<TMmio> {
    /// # Safety
    ///
    /// The caller is responsible for ensuring that ptr is valid for
    /// volatile reads and writes at any of the offsets in this register
    /// block.
    #[inline(always)]
    pub unsafe fn new(ptr: *mut u32) -> Self {
        Self {
            ptr,
            mmio: core::default::Default::default(),
        }
    }
}
impl<TMmio: ureg::Mmio> RegisterBlock<TMmio> {
    /// # Safety
    ///
    /// The caller is responsible for ensuring that ptr is valid for
    /// volatile reads and writes at any of the offsets in this register
    /// block.
    #[inline(always)]
    pub unsafe fn new_with_mmio(ptr: *mut u32, mmio: TMmio) -> Self {
        Self { ptr, mmio }
    }
    /// Read value: [`u32`]; Write value: [`u32`]
    #[inline(always)]
    pub fn key_share0(&self) -> ureg::Array<8, ureg::RegRef<crate::aes::meta::KeyShare0, &TMmio>> {
        unsafe {
            ureg::Array::new_with_mmio(
                self.ptr.wrapping_add(4 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`u32`]; Write value: [`u32`]
    #[inline(always)]
    pub fn key_share1(&self) -> ureg::Array<8, ureg::RegRef<crate::aes::meta::KeyShare1, &TMmio>> {
        unsafe {
            ureg::Array::new_with_mmio(
                self.ptr.wrapping_add(0x24 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`u32`]; Write value: [`u32`]
    #[inline(always)]
    pub fn iv(&self) -> ureg::Array<4, ureg::RegRef<crate::aes::meta::Iv, &TMmio>> {
        unsafe {
            ureg::Array::new_with_mmio(
                self.ptr.wrapping_add(0x44 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`u32`]; Write value: [`u32`]
    #[inline(always)]
    pub fn data_in(&self) -> ureg::Array<4, ureg::RegRef<crate::aes::meta::DataIn, &TMmio>> {
        unsafe {
            ureg::Array::new_with_mmio(
                self.ptr.wrapping_add(0x54 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`u32`]; Write value: [`u32`]
    #[inline(always)]
    pub fn data_out(&self) -> ureg::Array<4, ureg::RegRef<crate::aes::meta::DataOut, &TMmio>> {
        unsafe {
            ureg::Array::new_with_mmio(
                self.ptr.wrapping_add(0x64 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`aes::regs::CtrlShadowedReadVal`]; Write value: [`aes::regs::CtrlShadowedWriteVal`]
    #[inline(always)]
    pub fn ctrl_shadowed(&self) -> ureg::RegRef<crate::aes::meta::CtrlShadowed, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0x74 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`aes::regs::CtrlAuxShadowedReadVal`]; Write value: [`aes::regs::CtrlAuxShadowedWriteVal`]
    #[inline(always)]
    pub fn ctrl_aux_shadowed(&self) -> ureg::RegRef<crate::aes::meta::CtrlAuxShadowed, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0x78 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`aes::regs::CtrlAuxRegwenReadVal`]; Write value: [`aes::regs::CtrlAuxRegwenWriteVal`]
    #[inline(always)]
    pub fn ctrl_aux_regwen(&self) -> ureg::RegRef<crate::aes::meta::CtrlAuxRegwen, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0x7c / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`aes::regs::TriggerReadVal`]; Write value: [`aes::regs::TriggerWriteVal`]
    #[inline(always)]
    pub fn trigger(&self) -> ureg::RegRef<crate::aes::meta::Trigger, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0x80 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`aes::regs::StatusReadVal`]; Write value: [`aes::regs::StatusWriteVal`]
    #[inline(always)]
    pub fn status(&self) -> ureg::RegRef<crate::aes::meta::Status, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0x84 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// Read value: [`aes::regs::CtrlGcmShadowedReadVal`]; Write value: [`aes::regs::CtrlGcmShadowedWriteVal`]
    #[inline(always)]
    pub fn ctrl_gcm_shadowed(&self) -> ureg::RegRef<crate::aes::meta::CtrlGcmShadowed, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0x88 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
}
pub mod regs {
    //! Types that represent the values held by registers.
    #[derive(Clone, Copy)]
    pub struct CtrlAuxRegwenReadVal(u32);
    impl CtrlAuxRegwenReadVal {
        /// Auxiliary Control Register configuration enable
        /// bit.  If this is cleared to 0, the Auxiliary Control
        /// Register cannot be written anymore.
        #[inline(always)]
        pub fn ctrl_aux_regwen(&self) -> bool {
            ((self.0 >> 0) & 1) != 0
        }
        /// Construct a WriteVal that can be used to modify the contents of this register value.
        #[inline(always)]
        pub fn modify(self) -> CtrlAuxRegwenWriteVal {
            CtrlAuxRegwenWriteVal(self.0)
        }
    }
    impl From<u32> for CtrlAuxRegwenReadVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlAuxRegwenReadVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlAuxRegwenReadVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlAuxRegwenWriteVal(u32);
    impl CtrlAuxRegwenWriteVal {
        /// Auxiliary Control Register configuration enable
        /// bit.  If this is cleared to 0, the Auxiliary Control
        /// Register cannot be written anymore.
        #[inline(always)]
        pub fn ctrl_aux_regwen(self, val: bool) -> Self {
            Self((self.0 & !(1 << 0)) | (u32::from(val) << 0))
        }
    }
    impl From<u32> for CtrlAuxRegwenWriteVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlAuxRegwenWriteVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlAuxRegwenWriteVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlAuxShadowedReadVal(u32);
    impl CtrlAuxShadowedReadVal {
        /// Controls whether providing a new key triggers the reseeding
        /// of internal pseudo-random number generators used for clearing and
        /// masking (1) or not (0).
        #[inline(always)]
        pub fn key_touch_forces_reseed(&self) -> bool {
            ((self.0 >> 0) & 1) != 0
        }
        /// Allow the internal masking PRNG to advance (0) or
        /// force its internal state (1) leading to constant masks.
        /// Setting all masks to constant value can be useful when
        /// performing SCA.  To completely disable the masking, the
        /// second key share (KEY_SHARE1_0 - KEY_SHARE1_7) must be
        /// zero as well.  In addition, a special seed needs to be
        /// loaded into the masking PRNG using the EDN interface.
        /// Only applicable if both the Masking parameter and the
        /// SecAllowForcingMasks parameter are set to one.
        #[inline(always)]
        pub fn force_masks(&self) -> bool {
            ((self.0 >> 1) & 1) != 0
        }
        /// Construct a WriteVal that can be used to modify the contents of this register value.
        #[inline(always)]
        pub fn modify(self) -> CtrlAuxShadowedWriteVal {
            CtrlAuxShadowedWriteVal(self.0)
        }
    }
    impl From<u32> for CtrlAuxShadowedReadVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlAuxShadowedReadVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlAuxShadowedReadVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlAuxShadowedWriteVal(u32);
    impl CtrlAuxShadowedWriteVal {
        /// Controls whether providing a new key triggers the reseeding
        /// of internal pseudo-random number generators used for clearing and
        /// masking (1) or not (0).
        #[inline(always)]
        pub fn key_touch_forces_reseed(self, val: bool) -> Self {
            Self((self.0 & !(1 << 0)) | (u32::from(val) << 0))
        }
        /// Allow the internal masking PRNG to advance (0) or
        /// force its internal state (1) leading to constant masks.
        /// Setting all masks to constant value can be useful when
        /// performing SCA.  To completely disable the masking, the
        /// second key share (KEY_SHARE1_0 - KEY_SHARE1_7) must be
        /// zero as well.  In addition, a special seed needs to be
        /// loaded into the masking PRNG using the EDN interface.
        /// Only applicable if both the Masking parameter and the
        /// SecAllowForcingMasks parameter are set to one.
        #[inline(always)]
        pub fn force_masks(self, val: bool) -> Self {
            Self((self.0 & !(1 << 1)) | (u32::from(val) << 1))
        }
    }
    impl From<u32> for CtrlAuxShadowedWriteVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlAuxShadowedWriteVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlAuxShadowedWriteVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlGcmShadowedReadVal(u32);
    impl CtrlGcmShadowedReadVal {
        /// 6-bit one-hot field to select the phase of the
        /// Galois/Counter Mode (GCM) of operation.  Invalid input
        /// values, i.e., values with multiple bits set and value
        /// 6'b00_0000, are mapped to GCM_INIT (6'b00_0001).  In case
        /// support for GCM has been disabled at compile time, this
        /// field is not writable and always reads as GCM_INIT
        /// (6'b00_0001).
        #[inline(always)]
        pub fn phase(&self) -> u32 {
            (self.0 >> 0) & 0x3f
        }
        /// Number of valid bytes of the current input block.
        /// Only the last block in the GCM_AAD and GCM_TEXT phases are
        /// expected to have not all bytes marked as valid.  For all
        /// other blocks, the number of valid bytes should be set to 16.
        /// Invalid input values, i.e., the value 5'b0_0000, and all
        /// other values different from 5'b1_0000 in case GCM is not
        /// supported (because disabled at compile time) are mapped to
        /// 5'b1_0000.
        #[inline(always)]
        pub fn num_valid_bytes(&self) -> u32 {
            (self.0 >> 6) & 0x1f
        }
        /// Construct a WriteVal that can be used to modify the contents of this register value.
        #[inline(always)]
        pub fn modify(self) -> CtrlGcmShadowedWriteVal {
            CtrlGcmShadowedWriteVal(self.0)
        }
    }
    impl From<u32> for CtrlGcmShadowedReadVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlGcmShadowedReadVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlGcmShadowedReadVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlGcmShadowedWriteVal(u32);
    impl CtrlGcmShadowedWriteVal {
        /// 6-bit one-hot field to select the phase of the
        /// Galois/Counter Mode (GCM) of operation.  Invalid input
        /// values, i.e., values with multiple bits set and value
        /// 6'b00_0000, are mapped to GCM_INIT (6'b00_0001).  In case
        /// support for GCM has been disabled at compile time, this
        /// field is not writable and always reads as GCM_INIT
        /// (6'b00_0001).
        #[inline(always)]
        pub fn phase(self, val: u32) -> Self {
            Self((self.0 & !(0x3f << 0)) | ((val & 0x3f) << 0))
        }
        /// Number of valid bytes of the current input block.
        /// Only the last block in the GCM_AAD and GCM_TEXT phases are
        /// expected to have not all bytes marked as valid.  For all
        /// other blocks, the number of valid bytes should be set to 16.
        /// Invalid input values, i.e., the value 5'b0_0000, and all
        /// other values different from 5'b1_0000 in case GCM is not
        /// supported (because disabled at compile time) are mapped to
        /// 5'b1_0000.
        #[inline(always)]
        pub fn num_valid_bytes(self, val: u32) -> Self {
            Self((self.0 & !(0x1f << 6)) | ((val & 0x1f) << 6))
        }
    }
    impl From<u32> for CtrlGcmShadowedWriteVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlGcmShadowedWriteVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlGcmShadowedWriteVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlShadowedReadVal(u32);
    impl CtrlShadowedReadVal {
        /// 2-bit one-hot field to select the operation of AES
        /// unit.  Invalid input values, i.e., values with multiple
        /// bits set and value 2'b00, are mapped to AES_ENC (2'b01).
        #[inline(always)]
        pub fn operation(&self) -> u32 {
            (self.0 >> 0) & 3
        }
        /// 6-bit one-hot field to select AES block cipher
        /// mode.  Invalid input values, i.e., values with multiple
        /// bits set and value 6'b00_0000, are mapped to AES_NONE
        /// (6'b11_1111).
        #[inline(always)]
        pub fn mode(&self) -> u32 {
            (self.0 >> 2) & 0x3f
        }
        /// 3-bit one-hot field to select AES key length.
        /// Invalid input values, i.e., values with multiple bits set,
        /// value 3'b000, and value 3'b010 in case 192-bit keys are
        /// not supported (because disabled at compile time) are
        /// mapped to AES_256 (3'b100).
        #[inline(always)]
        pub fn key_len(&self) -> u32 {
            (self.0 >> 8) & 7
        }
        /// Controls whether the AES unit uses the key
        /// provided by the key manager via key sideload interface (1)
        /// or the key provided by software via Initial Key Registers
        /// KEY_SHARE1_0 - KEY_SHARE1_7 (0).
        #[inline(always)]
        pub fn sideload(&self) -> bool {
            ((self.0 >> 11) & 1) != 0
        }
        /// 3-bit one-hot field to control the reseeding rate
        /// of the internal pseudo-random number generator (PRNG) used
        /// for masking. Invalid input values, i.e., values with
        /// multiple bits set and value 3'b000 are mapped to the
        /// highest reseeding rate PER_1 (3'b001).
        #[inline(always)]
        pub fn prng_reseed_rate(&self) -> u32 {
            (self.0 >> 12) & 7
        }
        /// Controls whether the AES unit is operated in
        /// normal/automatic mode (0) or fully manual mode (1).  In
        /// automatic mode (0), the AES unit automatically i) starts
        /// to encrypt/decrypt when it receives new input data, and
        /// ii) stalls during the last encryption/decryption cycle if
        /// the previous output data has not yet been read.  This is
        /// the most efficient mode to operate in.  Note that the
        /// corresponding status tracking is automatically cleared
        /// upon a write to the Control Register.  In manual mode (1),
        /// the AES unit i) only starts to encrypt/decrypt after
        /// receiving a start trigger (see Trigger Register), and ii)
        /// overwrites previous output data irrespective of whether it
        /// has been read out or not.  This mode is useful if software needs full
        /// control over the AES unit.
        #[inline(always)]
        pub fn manual_operation(&self) -> bool {
            ((self.0 >> 15) & 1) != 0
        }
        /// Construct a WriteVal that can be used to modify the contents of this register value.
        #[inline(always)]
        pub fn modify(self) -> CtrlShadowedWriteVal {
            CtrlShadowedWriteVal(self.0)
        }
    }
    impl From<u32> for CtrlShadowedReadVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlShadowedReadVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlShadowedReadVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CtrlShadowedWriteVal(u32);
    impl CtrlShadowedWriteVal {
        /// 2-bit one-hot field to select the operation of AES
        /// unit.  Invalid input values, i.e., values with multiple
        /// bits set and value 2'b00, are mapped to AES_ENC (2'b01).
        #[inline(always)]
        pub fn operation(self, val: u32) -> Self {
            Self((self.0 & !(3 << 0)) | ((val & 3) << 0))
        }
        /// 6-bit one-hot field to select AES block cipher
        /// mode.  Invalid input values, i.e., values with multiple
        /// bits set and value 6'b00_0000, are mapped to AES_NONE
        /// (6'b11_1111).
        #[inline(always)]
        pub fn mode(self, val: u32) -> Self {
            Self((self.0 & !(0x3f << 2)) | ((val & 0x3f) << 2))
        }
        /// 3-bit one-hot field to select AES key length.
        /// Invalid input values, i.e., values with multiple bits set,
        /// value 3'b000, and value 3'b010 in case 192-bit keys are
        /// not supported (because disabled at compile time) are
        /// mapped to AES_256 (3'b100).
        #[inline(always)]
        pub fn key_len(self, val: u32) -> Self {
            Self((self.0 & !(7 << 8)) | ((val & 7) << 8))
        }
        /// Controls whether the AES unit uses the key
        /// provided by the key manager via key sideload interface (1)
        /// or the key provided by software via Initial Key Registers
        /// KEY_SHARE1_0 - KEY_SHARE1_7 (0).
        #[inline(always)]
        pub fn sideload(self, val: bool) -> Self {
            Self((self.0 & !(1 << 11)) | (u32::from(val) << 11))
        }
        /// 3-bit one-hot field to control the reseeding rate
        /// of the internal pseudo-random number generator (PRNG) used
        /// for masking. Invalid input values, i.e., values with
        /// multiple bits set and value 3'b000 are mapped to the
        /// highest reseeding rate PER_1 (3'b001).
        #[inline(always)]
        pub fn prng_reseed_rate(self, val: u32) -> Self {
            Self((self.0 & !(7 << 12)) | ((val & 7) << 12))
        }
        /// Controls whether the AES unit is operated in
        /// normal/automatic mode (0) or fully manual mode (1).  In
        /// automatic mode (0), the AES unit automatically i) starts
        /// to encrypt/decrypt when it receives new input data, and
        /// ii) stalls during the last encryption/decryption cycle if
        /// the previous output data has not yet been read.  This is
        /// the most efficient mode to operate in.  Note that the
        /// corresponding status tracking is automatically cleared
        /// upon a write to the Control Register.  In manual mode (1),
        /// the AES unit i) only starts to encrypt/decrypt after
        /// receiving a start trigger (see Trigger Register), and ii)
        /// overwrites previous output data irrespective of whether it
        /// has been read out or not.  This mode is useful if software needs full
        /// control over the AES unit.
        #[inline(always)]
        pub fn manual_operation(self, val: bool) -> Self {
            Self((self.0 & !(1 << 15)) | (u32::from(val) << 15))
        }
    }
    impl From<u32> for CtrlShadowedWriteVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CtrlShadowedWriteVal> for u32 {
        #[inline(always)]
        fn from(val: CtrlShadowedWriteVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct StatusReadVal(u32);
    impl StatusReadVal {
        /// The AES unit is idle (1) or busy (0).  This flag
        /// is `0` if one of the following operations is currently
        /// running: i) encryption/decryption, ii) register clearing or
        /// iii) PRNG reseeding.  This flag is also `0` if an
        /// encryption/decryption is running but the AES unit is
        /// stalled.
        #[inline(always)]
        pub fn idle(&self) -> bool {
            ((self.0 >> 0) & 1) != 0
        }
        /// The AES unit is not stalled (0) or stalled (1)
        /// because there is previous output data that must be read by
        /// the processor before the AES unit can overwrite this data.
        /// This flag is not meaningful if MANUAL_OPERATION=1 (see
        /// Control Register).
        #[inline(always)]
        pub fn stall(&self) -> bool {
            ((self.0 >> 1) & 1) != 0
        }
        /// All previous output data has been fully read by
        /// the processor (0) or at least one previous output data block
        /// has been lost (1).  It has been overwritten by the AES unit
        /// before the processor could fully read it.  Once set to `1`,
        /// this flag remains set until AES operation is restarted by
        /// re-writing the Control Register.  The primary use of this
        /// flag is for design verification.  This flag is not
        /// meaningful if MANUAL_OPERATION=0 (see Control Register).
        #[inline(always)]
        pub fn output_lost(&self) -> bool {
            ((self.0 >> 2) & 1) != 0
        }
        /// The AES unit has no valid output (0) or has valid output data (1).
        #[inline(always)]
        pub fn output_valid(&self) -> bool {
            ((self.0 >> 3) & 1) != 0
        }
        /// The AES unit is ready (1) or not ready (0) to
        /// receive new data input via the DATA_IN registers.  If the
        /// present values in the DATA_IN registers have not yet been
        /// loaded into the module this flag is `0` (not ready).
        #[inline(always)]
        pub fn input_ready(&self) -> bool {
            ((self.0 >> 4) & 1) != 0
        }
        /// An update error has not occurred (0) or has
        /// occurred (1) in the shadowed Control Register.  AES
        /// operation needs to be restarted by re-writing the Control
        /// Register.
        #[inline(always)]
        pub fn alert_recov_ctrl_update_err(&self) -> bool {
            ((self.0 >> 5) & 1) != 0
        }
        /// No fatal fault has occurred inside the AES unit
        /// (0).  A fatal fault has occurred and the AES unit needs to
        /// be reset (1).  Examples for fatal faults include i) storage
        /// errors in the Control Register, ii) if any internal FSM
        /// enters an invalid state, iii) if any sparsely encoded signal
        /// takes on an invalid value, iv) errors in the internal round
        /// counter, v) escalations triggered by the life cycle
        /// controller, and vi) fatal integrity failures on the TL-UL bus.
        #[inline(always)]
        pub fn alert_fatal_fault(&self) -> bool {
            ((self.0 >> 6) & 1) != 0
        }
    }
    impl From<u32> for StatusReadVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<StatusReadVal> for u32 {
        #[inline(always)]
        fn from(val: StatusReadVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct TriggerWriteVal(u32);
    impl TriggerWriteVal {
        /// Keep AES unit paused (0) or trigger the
        /// encryption/decryption of one data block (1).  This trigger
        /// is cleared to `0` if MANUAL_OPERATION=0 or if MODE=AES_NONE
        /// (see Control Register).
        #[inline(always)]
        pub fn start(self, val: bool) -> Self {
            Self((self.0 & !(1 << 0)) | (u32::from(val) << 0))
        }
        /// Keep current values in Initial Key, internal Full
        /// Key and Decryption Key registers, IV registers and Input
        /// Data registers (0) or clear all those registers with
        /// pseudo-random data (1).
        #[inline(always)]
        pub fn key_iv_data_in_clear(self, val: bool) -> Self {
            Self((self.0 & !(1 << 1)) | (u32::from(val) << 1))
        }
        /// Keep current values in Output Data registers (0) or
        /// clear those registers with pseudo-random data (1).
        #[inline(always)]
        pub fn data_out_clear(self, val: bool) -> Self {
            Self((self.0 & !(1 << 2)) | (u32::from(val) << 2))
        }
        /// Keep continuing with the current states of the
        /// internal pseudo-random number generators used for register
        /// clearing and masking (0) or perform a reseed of the internal
        /// states from the connected entropy source (1).  If the
        /// KEY_TOUCH_FORCES_RESEED bit in the Auxiliary Control
        /// Register is set to one, this trigger will automatically get
        /// set after providing a new initial key.
        #[inline(always)]
        pub fn prng_reseed(self, val: bool) -> Self {
            Self((self.0 & !(1 << 3)) | (u32::from(val) << 3))
        }
    }
    impl From<u32> for TriggerWriteVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<TriggerWriteVal> for u32 {
        #[inline(always)]
        fn from(val: TriggerWriteVal) -> u32 {
            val.0
        }
    }
}
pub mod enums {
    //! Enumerations used by some register fields.
    pub mod selector {}
}
pub mod meta {
    //! Additional metadata needed by ureg.
    pub type KeyShare0 = ureg::WriteOnlyReg32<0, u32>;
    pub type KeyShare1 = ureg::WriteOnlyReg32<0, u32>;
    pub type Iv = ureg::ReadWriteReg32<0, u32, u32>;
    pub type DataIn = ureg::WriteOnlyReg32<0, u32>;
    pub type DataOut = ureg::ReadOnlyReg32<u32>;
    pub type CtrlShadowed = ureg::ReadWriteReg32<
        0,
        crate::aes::regs::CtrlShadowedReadVal,
        crate::aes::regs::CtrlShadowedWriteVal,
    >;
    pub type CtrlAuxShadowed = ureg::ReadWriteReg32<
        0,
        crate::aes::regs::CtrlAuxShadowedReadVal,
        crate::aes::regs::CtrlAuxShadowedWriteVal,
    >;
    pub type CtrlAuxRegwen = ureg::ReadWriteReg32<
        0,
        crate::aes::regs::CtrlAuxRegwenReadVal,
        crate::aes::regs::CtrlAuxRegwenWriteVal,
    >;
    pub type Trigger = ureg::WriteOnlyReg32<0, crate::aes::regs::TriggerWriteVal>;
    pub type Status = ureg::ReadOnlyReg32<crate::aes::regs::StatusReadVal>;
    pub type CtrlGcmShadowed = ureg::ReadWriteReg32<
        0,
        crate::aes::regs::CtrlGcmShadowedReadVal,
        crate::aes::regs::CtrlGcmShadowedWriteVal,
    >;
}
