use std::{fs::File, io::BufReader, path::Path};

pub type RustlsConfig = rustls::ServerConfig;

pub fn create_rustls_config(
    tls_certificate_path: &Path,
    tls_private_key_path: &Path,
) -> anyhow::Result<RustlsConfig> {
    let certs = {
        let f = File::open(tls_certificate_path)?;
        let mut f = BufReader::new(f);

        rustls_pemfile::certs(&mut f)?
            .into_iter()
            .map(|bytes| rustls::Certificate(bytes))
            .collect()
    };

    let private_key = {
        let f = File::open(tls_private_key_path)?;
        let mut f = BufReader::new(f);

        rustls_pemfile::pkcs8_private_keys(&mut f)?
            .first()
            .map(|bytes| rustls::PrivateKey(bytes.clone()))
            .ok_or(anyhow::anyhow!("No private keys in file"))?
    };

    let tls_config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)?;

    Ok(tls_config)
}
