/*++

Licensed under the Apache-2.0 license.

File Name:

    lib.rs

Abstract:

    File contains exports for the Caliptra Known Answer Tests.

--*/

#![no_std]

mod ecc384_kat;
mod hmac_kdf_kat;
mod kats_env;
mod lms_kat;
mod mldsa87_kat;
mod sha1_kat;
mod sha256_kat;
mod sha2_512_384acc_kat;
mod sha384_kat;

pub use caliptra_drivers::{CaliptraError, CaliptraResult};
pub use ecc384_kat::Ecc384Kat;
pub use hmac_kdf_kat::{Hmac384KdfKat, Hmac512KdfKat};
pub use kats_env::KatsEnv;
pub use lms_kat::LmsKat;
pub use mldsa87_kat::Mldsa87Kat;
pub use sha1_kat::Sha1Kat;
pub use sha256_kat::Sha256Kat;
pub use sha2_512_384acc_kat::Sha2_512_384AccKat;
pub use sha384_kat::Sha384Kat;

use caliptra_drivers::cprintln;

/// Execute Known Answer Tests
///
/// # Arguments
///
/// * `env` - ROM Environment
pub fn execute_kat(env: &mut KatsEnv) -> CaliptraResult<()> {
    cprintln!("[kat] ++");

    cprintln!("[kat] sha1");
    Sha1Kat::default().execute(env.sha1)?;

    cprintln!("[kat] SHA2-256");
    Sha256Kat::default().execute(env.sha256)?;

    cprintln!("[kat] SHA2-384");
    Sha384Kat::default().execute(env.sha2_512_384)?;

    cprintln!("[kat] SHA2-512-ACC");
    Sha2_512_384AccKat::default().execute(env.sha2_512_384_acc, env.sha_acc_lock_state)?;

    cprintln!("[kat] ECC-384");
    Ecc384Kat::default().execute(env.ecc384, env.trng)?;

    cprintln!("[kat] HMAC-384Kdf");
    Hmac384KdfKat::default().execute(env.hmac, env.trng)?;

    cprintln!("[kat] HMAC-512Kdf");
    Hmac512KdfKat::default().execute(env.hmac, env.trng)?;

    cprintln!("[kat] LMS");
    LmsKat::default().execute(env.sha256, env.lms)?;

    cprintln!("[kat] MLDSA87");
    Mldsa87Kat::default().execute(env.mldsa87, env.trng)?;

    cprintln!("[kat] --");

    Ok(())
}
