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
pub struct SocIfcTrngReg {
    _priv: (),
}
impl SocIfcTrngReg {
    pub const PTR: *mut u32 = 0x30030000 as *mut u32;
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
    /// Storage for the requested TRNG Data.
    /// [br]Caliptra Access: RO
    /// [br]SOC Access:      RW
    ///
    /// Read value: [`u32`]; Write value: [`u32`]
    #[inline(always)]
    pub fn cptra_trng_data(
        &self,
    ) -> ureg::Array<12, ureg::RegRef<crate::soc_ifc_trng::meta::CptraTrngData, &TMmio>> {
        unsafe {
            ureg::Array::new_with_mmio(
                self.ptr.wrapping_add(0x78 / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
    /// TRNG Status register to indicate request and done
    ///
    /// Read value: [`soc_ifc_trng::regs::CptraTrngStatusReadVal`]; Write value: [`soc_ifc_trng::regs::CptraTrngStatusWriteVal`]
    #[inline(always)]
    pub fn cptra_trng_status(
        &self,
    ) -> ureg::RegRef<crate::soc_ifc_trng::meta::CptraTrngStatus, &TMmio> {
        unsafe {
            ureg::RegRef::new_with_mmio(
                self.ptr.wrapping_add(0xac / core::mem::size_of::<u32>()),
                core::borrow::Borrow::borrow(&self.mmio),
            )
        }
    }
}
pub mod regs {
    //! Types that represent the values held by registers.
    #[derive(Clone, Copy)]
    pub struct CptraTrngStatusReadVal(u32);
    impl CptraTrngStatusReadVal {
        /// Indicates that there is a request for TRNG Data.
        /// [br]Caliptra Access: RW
        /// [br]SOC Access:      RO
        #[inline(always)]
        pub fn data_req(&self) -> bool {
            ((self.0 >> 0) & 1) != 0
        }
        /// Indicates that the requests TRNG Data is done and stored in the TRNG Data register.
        /// [br]Caliptra Access: RO
        /// [br]SOC Access:      RW
        /// [br]When DATA_REQ is 0 DATA_WR_DONE will also be 0
        #[inline(always)]
        pub fn data_wr_done(&self) -> bool {
            ((self.0 >> 1) & 1) != 0
        }
        /// Construct a WriteVal that can be used to modify the contents of this register value.
        #[inline(always)]
        pub fn modify(self) -> CptraTrngStatusWriteVal {
            CptraTrngStatusWriteVal(self.0)
        }
    }
    impl From<u32> for CptraTrngStatusReadVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CptraTrngStatusReadVal> for u32 {
        #[inline(always)]
        fn from(val: CptraTrngStatusReadVal) -> u32 {
            val.0
        }
    }
    #[derive(Clone, Copy)]
    pub struct CptraTrngStatusWriteVal(u32);
    impl CptraTrngStatusWriteVal {
        /// Indicates that there is a request for TRNG Data.
        /// [br]Caliptra Access: RW
        /// [br]SOC Access:      RO
        #[inline(always)]
        pub fn data_req(self, val: bool) -> Self {
            Self((self.0 & !(1 << 0)) | (u32::from(val) << 0))
        }
        /// Indicates that the requests TRNG Data is done and stored in the TRNG Data register.
        /// [br]Caliptra Access: RO
        /// [br]SOC Access:      RW
        /// [br]When DATA_REQ is 0 DATA_WR_DONE will also be 0
        #[inline(always)]
        pub fn data_wr_done(self, val: bool) -> Self {
            Self((self.0 & !(1 << 1)) | (u32::from(val) << 1))
        }
    }
    impl From<u32> for CptraTrngStatusWriteVal {
        #[inline(always)]
        fn from(val: u32) -> Self {
            Self(val)
        }
    }
    impl From<CptraTrngStatusWriteVal> for u32 {
        #[inline(always)]
        fn from(val: CptraTrngStatusWriteVal) -> u32 {
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
    pub type CptraTrngData = ureg::ReadWriteReg32<0, u32, u32>;
    pub type CptraTrngStatus = ureg::ReadWriteReg32<
        0,
        crate::soc_ifc_trng::regs::CptraTrngStatusReadVal,
        crate::soc_ifc_trng::regs::CptraTrngStatusWriteVal,
    >;
}
