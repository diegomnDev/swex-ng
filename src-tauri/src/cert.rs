//! CA management. Equivalent of SWEX's node-forge usage: generate a root CA
//! once, persist it, and feed it to the MITM proxy (hudsucker) so it can sign
//! per-host leaf certs on the fly. The CA must be trusted by macOS (see
//! `macos::trust_certificate`).

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CertError {
    #[error("rcgen: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct CaFiles {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_path: PathBuf,
}

/// Load the CA from `dir` if present, otherwise generate a fresh root CA and
/// persist `ca.pem` / `ca.key`.
pub fn load_or_create_ca(dir: &Path) -> Result<CaFiles, CertError> {
    std::fs::create_dir_all(dir)?;
    let cert_path = dir.join("ca.pem");
    let key_path = dir.join("ca.key");

    if cert_path.exists() && key_path.exists() {
        return Ok(CaFiles {
            cert_pem: std::fs::read_to_string(&cert_path)?,
            key_pem: std::fs::read_to_string(&key_path)?,
            cert_path,
        });
    }

    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params
        .distinguished_name
        .push(DnType::CommonName, "SWEX-NG Root CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "SWEX-NG");

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    std::fs::write(&cert_path, &cert_pem)?;
    std::fs::write(&key_path, &key_pem)?;

    Ok(CaFiles {
        cert_pem,
        key_pem,
        cert_path,
    })
}
