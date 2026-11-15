//! Téléchargement du manifeste et de l'exécutable. HTTPS seulement (un
//! serveur local en clair est admis pour les tests), redirections comprises ;
//! tailles bornées avant et pendant la lecture ; empreinte SHA-256 calculée
//! au fil de l'écriture.
//!
//! Ces requêtes ne vont qu'à l'adresse de mise à jour et là où elle
//! redirige ([`redirect_allowed`]) : jamais vers Discord, sans jeton, sans
//! témoin, sans identifiant de l'utilisateur ni de la machine ; le seul
//! en-tête propre à Ferrocord est `User-Agent: Ferrocord/<version>`.

use super::manifest::{self, Manifest, MAX_BINARY, MAX_MANIFEST};
use aws_lc_rs::digest;
use std::io::Write;
use std::path::Path;

/// Adresse en clair sur cette machine (faux serveur des tests).
pub fn loopback(url: &str) -> bool {
    ["http://127.0.0.1:", "http://127.0.0.1/", "http://localhost:", "http://localhost/"].iter().any(|p| url.starts_with(p))
}

fn acceptable(url: &str) -> bool {
    url.starts_with("https://") || loopback(url)
}

/// Hôtes vers lesquels une redirection est suivie, en HTTPS (port 443) : les
/// « Releases » de GitHub renvoient vers son stockage de fichiers. Toute
/// autre redirection est refusée, sauf vers l'hôte même qui redirige.
pub const REDIRECT_HOSTS: [&str; 3] = ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];

/// Une redirection de `from` vers `to` est-elle suivie ?
/// - HTTPS vers un hôte de [`REDIRECT_HOSTS`], ou vers l'hôte de `from` ;
/// - en clair, seulement d'un serveur local vers le même hôte local (tests).
pub fn redirect_allowed(from: &reqwest::Url, to: &reqwest::Url) -> bool {
    let same_host = from.host_str().is_some() && from.host_str() == to.host_str();
    match to.scheme() {
        "https" => to.port_or_known_default() == Some(443) && to.host_str().is_some_and(|h| REDIRECT_HOSTS.contains(&h)) || same_host && from.scheme() == "https" && from.port_or_known_default() == to.port_or_known_default(),
        "http" => same_host && loopback(from.as_str()) && loopback(to.as_str()),
        _ => false,
    }
}

/// Erreur de reqwest avec ses causes (c'est là qu'est la raison d'une
/// redirection refusée), sans l'adresse.
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        let c_text = c.to_string();
        if !text.contains(&c_text) {
            text = format!("{text} : {c_text}");
        }
        cause = c.source();
    }
    text
}

pub fn client() -> Result<reqwest::Client, String> {
    let policy = reqwest::redirect::Policy::custom(|a| {
        if a.previous().len() >= 5 {
            return a.error("trop de redirections");
        }
        match a.previous().last() {
            Some(from) if redirect_allowed(from, a.url()) => a.follow(),
            _ => a.error("redirection vers un hôte non autorisé"),
        }
    });
    reqwest::Client::builder()
        .user_agent(concat!("Ferrocord/", env!("CARGO_PKG_VERSION")))
        .redirect(policy)
        // L'adresse d'où l'on vient n'est pas redite à l'hôte où l'on est renvoyé.
        .referer(false)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(client: &reqwest::Client, url: &str, max: u64) -> Result<reqwest::Response, String> {
    if !acceptable(url) {
        return Err("adresse refusée (HTTPS exigé)".into());
    }
    let resp = client.get(url).send().await.map_err(describe)?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > max) {
        return Err("réponse trop grosse".into());
    }
    Ok(resp)
}

/// Télécharge et vérifie le manifeste.
pub async fn manifest(client: &reqwest::Client, url: &str, key: &[u8; 32]) -> Result<Manifest, String> {
    let mut resp = get(client, url, MAX_MANIFEST as u64).await?;
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(describe)? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_MANIFEST {
            return Err("manifeste trop gros".into());
        }
    }
    manifest::verify(&body, key).map_err(|e| e.to_string())
}

/// Télécharge l'exécutable de `m` dans `dest`, en vérifiant taille et
/// SHA-256. `dest` est supprimé si quoi que ce soit ne va pas.
pub async fn binary(client: &reqwest::Client, m: &Manifest, dest: &Path, progress: &(dyn Fn(u64) + Send + Sync)) -> Result<(), String> {
    let result = async {
        let mut resp = get(client, &m.url, m.size.min(MAX_BINARY)).await?;
        let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        let mut hash = digest::Context::new(&digest::SHA256);
        let mut done = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(|e| format!("téléchargement interrompu ({})", describe(e)))? {
            done += chunk.len() as u64;
            if done > m.size {
                return Err("fichier plus gros qu'annoncé".to_string());
            }
            hash.update(&chunk);
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            progress(done);
        }
        if done != m.size {
            return Err("fichier incomplet".into());
        }
        if hash.finish().as_ref() != m.sha256 {
            return Err("empreinte SHA-256 différente du manifeste".into());
        }
        file.sync_all().map_err(|e| e.to_string())
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

/// SHA-256 d'un fichier (revérification au moment du remplacement).
pub fn file_sha256(path: &Path) -> std::io::Result<[u8; 32]> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hash = digest::Context::new(&digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.finish().as_ref());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serveur HTTP minimal sur 127.0.0.1 : rend `routes` (chemin → corps).
    async fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let (status, body) = routes.iter().find(|(p, _)| *p == path).map_or(("404 Not Found", Vec::new()), |(_, b)| ("200 OK", b.clone()));
                    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(&body).await;
                });
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    #[test]
    fn redirections_only_to_expected_hosts() {
        let u = |s: &str| reqwest::Url::parse(s).unwrap();
        let latest = u("https://github.com/moi/ferrocord/releases/latest/download/ferrocord-manifeste.json");
        // Le chemin réel d'une « Release » : github.com → github.com → stockage.
        assert!(redirect_allowed(&latest, &u("https://github.com/moi/ferrocord/releases/download/v1.0.0/ferrocord-manifeste.json")));
        assert!(redirect_allowed(&latest, &u("https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=x")));
        assert!(redirect_allowed(&latest, &u("https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2")));
        // Ailleurs, en clair, sur un autre port, ou un hôte qui ressemble : refusé.
        for to in ["https://exemple.invalid/f.exe", "http://github.com/moi/f.exe", "https://github.com:8443/f.exe", "https://github.com.exemple.invalid/f.exe", "https://raw.githubusercontent.com/moi/f.exe", "ftp://github.com/f.exe", "http://127.0.0.1:80/f.exe"] {
            assert!(!redirect_allowed(&latest, &u(to)), "{to}");
        }
        // Une adresse à soi (local.toml) peut rediriger chez elle, pas ailleurs.
        let own = u("https://exemple.org/maj/m.json");
        assert!(redirect_allowed(&own, &u("https://exemple.org/maj/v2/m.json")));
        assert!(!redirect_allowed(&own, &u("https://autre.exemple.org/m.json")));
        assert!(redirect_allowed(&own, &u("https://github.com/moi/f.exe")));
        // Serveur local des tests : vers lui-même seulement.
        let local = u("http://127.0.0.1:4000/m.json");
        assert!(redirect_allowed(&local, &u("http://127.0.0.1:4001/f.exe")));
        assert!(!redirect_allowed(&local, &u("http://localhost:4000/f.exe")));
        assert!(!redirect_allowed(&local, &u("http://192.0.2.1/f.exe")));
        assert!(!redirect_allowed(&local, &u("https://exemple.invalid/f.exe")));
    }

    #[tokio::test]
    async fn manifest_then_binary_are_checked() {
        let exe = b"MZ faux executable".to_vec();
        let sha: [u8; 32] = digest::digest(&digest::SHA256, &exe).as_ref().try_into().unwrap();
        let k = Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap().as_ref()).unwrap();
        let pk: [u8; 32] = k.public_key().as_ref().try_into().unwrap();
        // L'adresse du serveur n'est connue qu'après coup : deux serveurs.
        let bin = serve(vec![("/f.exe", exe.clone()), ("/autre.exe", b"MZ autre contenu!!".to_vec())]).await;
        let good = manifest::payload_json("9.0.0", &format!("{bin}/f.exe"), &sha, exe.len() as u64, "");
        let swapped = manifest::payload_json("9.0.0", &format!("{bin}/autre.exe"), &sha, exe.len() as u64, "");
        let sign = |p: &str| manifest::envelope_json(p, k.sign(p.as_bytes()).as_ref()).into_bytes();
        let base = serve(vec![("/m.json", sign(&good)), ("/m2.json", sign(&swapped))]).await;
        let c = client().unwrap();
        let m = manifest(&c, &format!("{base}/m.json"), &pk).await.unwrap();
        assert_eq!(m.version, manifest::Version(9, 0, 0));
        let dir = std::env::temp_dir().join(format!("ferrocord-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("ferrocord.new.exe");
        binary(&c, &m, &dest, &|_| {}).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), exe);
        assert_eq!(file_sha256(&dest).unwrap(), sha);
        // Autre fichier que celui du manifeste : refusé, rien ne reste.
        let m2 = manifest(&c, &format!("{base}/m2.json"), &pk).await.unwrap();
        assert!(binary(&c, &m2, &dest, &|_| {}).await.unwrap_err().contains("SHA-256"));
        assert!(!dest.exists());
        assert!(manifest(&c, &format!("{base}/absent.json"), &pk).await.unwrap_err().contains("404"));
        assert!(manifest(&c, "http://exemple.invalid/m.json", &pk).await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
