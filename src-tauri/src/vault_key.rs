//! Where the vault's data key comes from, and what has to happen before it
//! can be obtained at all.
//!
//! # What was wrong
//!
//! The data key was written to disk as **plain hex**. `0600` stops another
//! *user*; it stops nothing running as this one — a compromised dependency,
//! another app the owner ran, any process with the same privileges. The vault
//! protects identities from a stolen `vault.enc`, and a plaintext key file
//! beside it undoes exactly that.
//!
//! No location on disk fixes this. Whatever the app can read unattended, code
//! with the same privileges can read too. Only a secret the user supplies —
//! or hardware that demands their presence — closes it.
//!
//! # Shape
//!
//! The key file is now an envelope: a random salt, the KDF parameters used,
//! and the data key encrypted under a key derived from the user's passphrase.
//! Reading it without the passphrase yields ciphertext.
//!
//! ```text
//! passphrase ──Argon2id(salt, m=64MiB, t=3, p=1)──> KEK
//!                                                    │
//!            vault.key.enc { salt, params, nonce, ───┴─> AES-256-GCM ─> data key
//!                            wrapped }                                     │
//!                                                        vault.enc <───────┘
//! ```
//!
//! The parameters live **in the envelope** rather than in this file, so they
//! can be raised later without orphaning a vault written under the old ones.
//!
//! # Why the secret arrives through a handle
//!
//! [`cabal_vault::KeyProvider`] is synchronous and takes no arguments, and
//! widening it would push an unlock concern into every implementation. Instead
//! the provider holds an [`VaultUnlock`] that starts empty and is filled once,
//! by [`VaultUnlock::unlock`]. Before that, `data_key` reports the vault as
//! unavailable and every read of it fails closed.
//!
//! # What this does not defend
//!
//! A device stolen while unlocked, and malware running as this user *while the
//! app is unlocked* — it does not need the file, it can ask the running
//! process. This closes the at-rest hole. Hardware binding, which narrows the
//! rest, is a separate layer. See `docs/identity-design.md`.

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use cabal_vault::{DataKey, KeyProvider, Secret, VaultError};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use zeroize::Zeroize;

/// Argon2id parameters for new envelopes.
///
/// Asserted by test rather than merely written down: weakening them should be
/// a deliberate edit that fails the suite, not a typo nobody notices. See
/// `docs/identity-design.md` for the attacker these are chosen against.
pub const KDF_MEMORY_KIB: u32 = 64 * 1024;
pub const KDF_ITERATIONS: u32 = 3;
pub const KDF_PARALLELISM: u32 = 1;

const ENVELOPE_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const ARGON2ID: &str = "argon2id";

/// Failures before backoff starts, and the ceiling it grows to.
///
/// Not a security control: an attacker holding the file does not ask this app
/// for permission. It makes a borrowed-device attempt tedious, and nothing
/// more — see `docs/identity-design.md`, which says so in the same words so
/// this cannot later be read as if software counting were the defence.
const FREE_ATTEMPTS: u32 = 3;
/// The first backoff, doubling from there.
///
/// Longer than it looks like it needs to be, because a derivation already
/// costs a fraction of a second: a one-second penalty on top of that is not a
/// penalty, it is a rounding error on the work the attacker was doing anyway.
const BASE_BACKOFF_SECONDS: i64 = 5;
const MAX_BACKOFF_SECONDS: i64 = 15 * 60;

/// The key file as written to disk.
#[derive(Debug, Serialize, Deserialize)]
struct KeyEnvelope {
    version: u8,
    /// Which device store contributed the second half of the wrapping key.
    /// Recorded so a vault made with a binding is refused with a reason on a
    /// machine that has none, rather than failing as a wrong passphrase.
    #[serde(default = "unbound")]
    binding: String,
    /// Identifies which device secret wrapped this key, without revealing it.
    /// Empty when unbound. See `device_binding::fingerprint`.
    #[serde(default)]
    binding_check: Vec<u8>,
    kdf: KdfParams,
    /// Random per-write nonce for the wrapping cipher.
    nonce: Vec<u8>,
    /// The data key, encrypted under the passphrase-derived key.
    wrapped: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KdfParams {
    algorithm: String,
    salt: Vec<u8>,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

/// The binding written by builds that predate device binding.
fn unbound() -> String {
    crate::device_binding::Binding::None.as_str().to_string()
}

impl KdfParams {
    fn fresh() -> Result<Self, VaultError> {
        let mut salt = vec![0_u8; SALT_LEN];
        OsRng
            .try_fill_bytes(&mut salt)
            .map_err(|_| VaultError::KeyUnavailable)?;
        Ok(Self {
            algorithm: ARGON2ID.to_string(),
            salt,
            memory_kib: KDF_MEMORY_KIB,
            iterations: KDF_ITERATIONS,
            parallelism: KDF_PARALLELISM,
        })
    }

    /// Derives the key-encryption key for `secret`.
    ///
    /// Unknown algorithms are refused rather than approximated: deriving with
    /// the wrong function would produce a wrong key and report it as a wrong
    /// passphrase, sending the user to look for a mistake they did not make.
    ///
    /// `device_secret` is Argon2's "secret" parameter — a pepper. Mixed in
    /// here rather than hashed alongside afterwards because that is the
    /// construction the algorithm defines for exactly this, and inventing a
    /// combination step is how key derivations acquire subtle holes.
    fn derive(
        &self,
        secret: &str,
        device_secret: Option<&[u8]>,
    ) -> Result<[u8; 32], VaultError> {
        if self.algorithm != ARGON2ID {
            tracing::error!(
                target: "cabalmesh::vault",
                algorithm = %self.algorithm,
                "key envelope names an unsupported derivation"
            );
            return Err(VaultError::Malformed);
        }
        if self.salt.len() < SALT_LEN {
            return Err(VaultError::Malformed);
        }

        let params = argon2::Params::new(
            self.memory_kib,
            self.iterations,
            self.parallelism,
            Some(32),
        )
        .map_err(|_| VaultError::Malformed)?;

        let argon2 = match device_secret {
            Some(pepper) => argon2::Argon2::new_with_secret(
                pepper,
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                params,
            )
            .map_err(|_| VaultError::KeyUnavailable)?,
            None => argon2::Argon2::new(
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                params,
            ),
        };

        let mut derived = [0_u8; 32];
        argon2
            .hash_password_into(secret.as_bytes(), &self.salt, &mut derived)
            .map_err(|_| VaultError::KeyUnavailable)?;

        Ok(derived)
    }
}

/// Why an unlock attempt did not produce a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockFailure {
    /// The passphrase does not open this envelope. Nothing was changed.
    WrongSecret,
    /// Too many recent attempts; `retry_in_seconds` until the next is allowed.
    RateLimited { retry_in_seconds: i64 },
    /// The envelope is unreadable, or names a derivation this build lacks.
    /// Deliberately distinct from a wrong passphrase: one is the user's
    /// mistake and the other is not, and telling someone to retype a correct
    /// passphrase forever is its own kind of data loss.
    Unusable,
    /// This key is bound to a device store that is not available here —
    /// usually because the file was copied to another machine. The passphrase
    /// may well be right; it is not enough on its own, by design.
    DeviceBindingUnavailable,
}

/// Whether the vault can be opened at all yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultState {
    /// No envelope on disk. The next passphrase supplied creates one.
    Uninitialized,
    /// An envelope exists and no passphrase has been supplied this run.
    Locked,
    Unlocked,
}

impl VaultState {
    #[must_use]
    pub const fn is_unlocked(self) -> bool {
        matches!(self, Self::Unlocked)
    }
}

/// The shared slot the data key lives in once a passphrase has opened it.
///
/// Cloned into the provider and kept by the bridge, so the command that
/// unlocks and the vault that reads are talking about the same key without
/// either owning the other.
#[derive(Debug, Default)]
pub struct VaultUnlock {
    /// `None` until unlocked. Cached deliberately: Argon2id at these
    /// parameters takes a noticeable fraction of a second, and the vault is
    /// read on every identity operation.
    key: RwLock<Option<DataKey>>,
}

impl VaultUnlock {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.key.read().is_ok_and(|key| key.is_some())
    }

    /// Discards the cached key. The passphrase is required again.
    pub fn lock(&self) {
        if let Ok(mut key) = self.key.write() {
            *key = None;
        }
    }

    fn cached(&self) -> Option<DataKey> {
        self.key.read().ok().and_then(|key| key.clone())
    }

    fn store(&self, key: DataKey) {
        if let Ok(mut slot) = self.key.write() {
            *slot = Some(key);
        }
    }
}

/// A data key wrapped by the user's passphrase.
///
/// `Clone` shares the unlock slot rather than duplicating it: two handles on
/// the same envelope describe one vault, and unlocking through either opens
/// both. Cloning is how the bridge keeps a handle it can unlock while the
/// `Vault` owns the one it reads through.
#[derive(Clone)]
pub struct WrappedKeyProvider {
    /// The envelope.
    path: PathBuf,
    /// The pre-encryption plaintext key file, kept only to migrate away from.
    legacy_path: PathBuf,
    attempts_path: PathBuf,
    unlock: Arc<VaultUnlock>,
}

impl WrappedKeyProvider {
    /// A provider whose envelope lives at `path`.
    ///
    /// `legacy_path` is the plaintext key file this replaces; it is read once,
    /// during migration, and then removed.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, legacy_path: impl Into<PathBuf>, unlock: Arc<VaultUnlock>) -> Self {
        let path = path.into();
        let attempts_path = path.with_extension("attempts.json");
        Self {
            path,
            legacy_path: legacy_path.into(),
            attempts_path,
            unlock,
        }
    }

    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.unlock.is_unlocked()
    }

    /// Forgets the cached key. The passphrase is required again.
    pub fn lock(&self) {
        self.unlock.lock();
    }

    #[must_use]
    pub fn state(&self) -> VaultState {
        if self.unlock.is_unlocked() {
            VaultState::Unlocked
        } else if self.path.exists() || self.legacy_path.exists() {
            VaultState::Locked
        } else {
            VaultState::Uninitialized
        }
    }

    /// Supplies the passphrase, creating the envelope if there is none.
    ///
    /// # Errors
    ///
    /// [`UnlockFailure`] describes every refusal. None of them modify the
    /// stored key.
    pub fn unlock(&self, secret: &Secret) -> Result<(), UnlockFailure> {
        if let Some(retry_in_seconds) = self.throttled_for() {
            return Err(UnlockFailure::RateLimited { retry_in_seconds });
        }

        match self.open_or_create(secret) {
            Ok(key) => {
                self.clear_attempts();
                self.unlock.store(key);
                Ok(())
            }
            Err(UnlockFailure::WrongSecret) => {
                self.record_failure();
                Err(UnlockFailure::WrongSecret)
            }
            Err(other) => Err(other),
        }
    }

    fn open_or_create(&self, secret: &Secret) -> Result<DataKey, UnlockFailure> {
        if self.path.exists() {
            let key = self.open_envelope(secret)?;
            // An interrupted migration leaves both files. The envelope was
            // written *from* the legacy key, so equal bytes means the earlier
            // run got as far as writing and no further — finish it. Unequal
            // bytes means these are two different vaults, and deleting either
            // could be the deletion of a wallet, so both stay.
            self.retire_legacy_if_redundant(&key);
            return Ok(key);
        }

        if self.legacy_path.exists() {
            return self.migrate_legacy(secret);
        }

        self.create(secret)
    }

    fn create(&self, secret: &Secret) -> Result<DataKey, UnlockFailure> {
        let key = DataKey::generate().map_err(unusable)?;
        self.write_envelope(&key, secret)?;
        tracing::info!(
            target: "cabalmesh::vault",
            binding = crate::device_binding::platform_binding().as_str(),
            "created a passphrase-wrapped vault key"
        );
        Ok(key)
    }

    /// Adopts the plaintext key, then removes it — but only once the wrapped
    /// copy is proven to read back as the same key.
    ///
    /// The ordering is the whole point, and it is the same ordering
    /// [`cabal_vault::Vault::migrate_plaintext`] uses for the same reason:
    /// deleting first and failing second destroys a wallet.
    fn migrate_legacy(&self, secret: &Secret) -> Result<DataKey, UnlockFailure> {
        let key = read_legacy_key(&self.legacy_path).map_err(unusable)?;
        self.write_envelope(&key, secret)?;

        let round_tripped = self.open_envelope(secret)?;
        if round_tripped.expose_for_storage() != key.expose_for_storage() {
            tracing::error!(
                target: "cabalmesh::vault",
                "wrapped key did not read back identically; keeping the plaintext"
            );
            return Err(UnlockFailure::Unusable);
        }

        remove_secret_file(&self.legacy_path);
        tracing::info!(target: "cabalmesh::vault", "migrated the plaintext vault key into a passphrase-wrapped envelope");
        Ok(key)
    }

    fn retire_legacy_if_redundant(&self, key: &DataKey) {
        if !self.legacy_path.exists() {
            return;
        }
        match read_legacy_key(&self.legacy_path) {
            Ok(legacy) if legacy.expose_for_storage() == key.expose_for_storage() => {
                remove_secret_file(&self.legacy_path);
                tracing::info!(target: "cabalmesh::vault", "removed the redundant plaintext key left by an interrupted migration");
            }
            Ok(_) => tracing::error!(
                target: "cabalmesh::vault",
                "a plaintext key file holds a different key than the envelope; both kept, neither is safe to assume"
            ),
            Err(_) => tracing::warn!(
                target: "cabalmesh::vault",
                "a plaintext key file exists but is unreadable; leaving it untouched"
            ),
        }
    }

    fn open_envelope(&self, secret: &Secret) -> Result<DataKey, UnlockFailure> {
        let raw = std::fs::read_to_string(&self.path).map_err(|_| UnlockFailure::Unusable)?;
        let envelope: KeyEnvelope =
            serde_json::from_str(&raw).map_err(|_| UnlockFailure::Unusable)?;
        if envelope.version != ENVELOPE_VERSION || envelope.nonce.len() != NONCE_LEN {
            return Err(UnlockFailure::Unusable);
        }

        // Resolved before deriving anything. A key bound to a store this
        // machine does not have would otherwise derive a wrong KEK and be
        // reported as a wrong passphrase — sending someone to retype a
        // passphrase that was right the first time.
        let Some(binding) = crate::device_binding::Binding::parse(&envelope.binding) else {
            tracing::error!(
                target: "cabalmesh::vault",
                binding = %envelope.binding,
                "key envelope names a device binding this build does not know"
            );
            return Err(UnlockFailure::Unusable);
        };
        let device_secret = crate::device_binding::secret(binding)
            .map_err(|_| UnlockFailure::DeviceBindingUnavailable)?;

        // Checked before deriving. A store that answers with *a* secret, just
        // not the one this file was made under — the app directory restored
        // onto a second machine — is a different device, not a mistyped
        // passphrase, and saying so is the difference between "try your other
        // laptop" and "you have lost this wallet".
        let observed = crate::device_binding::fingerprint(
            &envelope.kdf.salt,
            device_secret.as_ref().map(|value| value.as_slice()),
        );
        if !envelope.binding_check.is_empty() && observed != envelope.binding_check {
            tracing::error!(
                target: "cabalmesh::vault",
                "this key belongs to a different device's key store"
            );
            return Err(UnlockFailure::DeviceBindingUnavailable);
        }

        let mut kek = envelope
            .kdf
            .derive(secret.expose(), device_secret.as_ref().map(|value| value.as_slice()))
            .map_err(unusable)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek));
        let opened = cipher.decrypt(
            Nonce::from_slice(&envelope.nonce),
            envelope.wrapped.as_ref(),
        );
        kek.zeroize();

        let mut bytes = opened.map_err(|_| UnlockFailure::WrongSecret)?;
        if bytes.len() != 32 {
            bytes.zeroize();
            return Err(UnlockFailure::Unusable);
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        bytes.zeroize();
        Ok(DataKey::from_bytes(key))
    }

    fn write_envelope(&self, key: &DataKey, secret: &Secret) -> Result<(), UnlockFailure> {
        // A refused store degrades to passphrase-only rather than making
        // the app unusable; the envelope records which one happened.
        let (binding, device_secret) = crate::device_binding::for_new_vault();

        let kdf = KdfParams::fresh().map_err(unusable)?;
        let mut kek = kdf
            .derive(secret.expose(), device_secret.as_ref().map(|value| value.as_slice()))
            .map_err(unusable)?;

        let mut nonce = [0_u8; NONCE_LEN];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| UnlockFailure::Unusable)?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek));
        let wrapped = cipher
            .encrypt(Nonce::from_slice(&nonce), key.expose_for_storage().as_ref())
            .map_err(|_| UnlockFailure::Unusable);
        kek.zeroize();

        let envelope = KeyEnvelope {
            version: ENVELOPE_VERSION,
            binding: binding.as_str().to_string(),
            binding_check: crate::device_binding::fingerprint(
                &kdf.salt,
                device_secret.as_ref().map(|value| value.as_slice()),
            ),
            kdf,
            nonce: nonce.to_vec(),
            wrapped: wrapped?,
        };

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| UnlockFailure::Unusable)?;
        }
        let encoded = serde_json::to_vec(&envelope).map_err(|_| UnlockFailure::Unusable)?;
        std::fs::File::create(&self.path).map_err(|_| UnlockFailure::Unusable)?;
        restrict_permissions(&self.path).map_err(|_| UnlockFailure::Unusable)?;
        std::fs::write(&self.path, &encoded).map_err(|_| UnlockFailure::Unusable)?;
        Ok(())
    }

    // ---- attempt throttling ------------------------------------------------

    /// Seconds until the next attempt is permitted, if one is not yet.
    fn throttled_for(&self) -> Option<i64> {
        let record = self.attempts()?;
        let remaining = (record.next_attempt_at - Utc::now()).num_seconds();
        (remaining > 0).then_some(remaining)
    }

    fn attempts(&self) -> Option<AttemptRecord> {
        std::fs::read_to_string(&self.attempts_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
    }

    fn record_failure(&self) {
        let failures = self.attempts().map_or(1, |record| record.failures + 1);
        // Backoff only after a few genuine typing mistakes, then doubling to a
        // ceiling so a forgetful owner is inconvenienced rather than locked
        // out for a day.
        let delay = if failures <= FREE_ATTEMPTS {
            0
        } else {
            BASE_BACKOFF_SECONDS
                .saturating_mul(1_i64 << (failures - FREE_ATTEMPTS - 1).min(20))
                .min(MAX_BACKOFF_SECONDS)
        };
        let record = AttemptRecord {
            failures,
            next_attempt_at: Utc::now() + Duration::seconds(delay),
        };
        if let Ok(encoded) = serde_json::to_vec(&record) {
            let _ = std::fs::write(&self.attempts_path, encoded);
        }
    }

    fn clear_attempts(&self) {
        let _ = std::fs::remove_file(&self.attempts_path);
    }
}

/// Persisted so a restart does not reset the count. See [`FREE_ATTEMPTS`] for
/// what this is and is not.
#[derive(Debug, Serialize, Deserialize)]
struct AttemptRecord {
    failures: u32,
    next_attempt_at: DateTime<Utc>,
}

impl KeyProvider for WrappedKeyProvider {
    fn data_key(&self) -> Result<DataKey, VaultError> {
        // Fails closed. Before a passphrase arrives there is no key, and the
        // vault must report that rather than inventing one — generating a
        // fresh key here would orphan every identity already stored.
        self.unlock.cached().ok_or(VaultError::KeyUnavailable)
    }

    fn describe(&self) -> &'static str {
        "passphrase-wrapped"
    }
}

/// Everything that is not the user's mistake reads the same to them: the
/// stored key cannot be opened, and retyping will not change that.
fn unusable(error: VaultError) -> UnlockFailure {
    tracing::error!(target: "cabalmesh::vault", %error, "vault key could not be opened");
    UnlockFailure::Unusable
}

fn read_legacy_key(path: &Path) -> Result<DataKey, VaultError> {
    let mut encoded = std::fs::read_to_string(path).map_err(|_| VaultError::KeyUnavailable)?;
    let mut bytes = [0_u8; 32];
    let decoded = hex::decode_to_slice(encoded.trim(), &mut bytes);
    encoded.zeroize();
    decoded.map_err(|_| VaultError::Malformed)?;
    Ok(DataKey::from_bytes(bytes))
}

/// Removes a file that held key material, logging what happened.
///
/// A failure here leaves plaintext on disk, which is the condition this whole
/// module exists to end — so it is reported loudly rather than swallowed.
fn remove_secret_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        tracing::error!(
            target: "cabalmesh::vault",
            %error,
            "could not remove a plaintext key file; it is still on disk"
        );
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // 0600 before anything is written into it. The envelope is ciphertext, so
    // this is no longer the protection — it is the last one to give up.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> std::io::Result<()> {
    // Windows inherits the user's profile ACL, which is already owner-scoped
    // for the app data directory.
    Ok(())
}

/// The provider for this platform.
///
/// There is only one today. The device key store — which would bind this key
/// to the hardware as a second layer under the passphrase — is a separate
/// ticket, and until it lands, claiming per-platform behaviour here would be
/// claiming protection that does not exist.
pub fn platform_provider(key_path: PathBuf, unlock: Arc<VaultUnlock>) -> WrappedKeyProvider {
    let legacy_path = key_path.clone();
    let envelope_path = key_path.with_extension("key.enc");
    WrappedKeyProvider::new(envelope_path, legacy_path, unlock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_provider(dir: &TempDir) -> (WrappedKeyProvider, Arc<VaultUnlock>) {
        let unlock = VaultUnlock::new();
        let provider = platform_provider(dir.path().join("vault.key"), unlock.clone());
        (provider, unlock)
    }

    fn secret(value: &str) -> Secret {
        Secret::new(value)
    }

    #[test]
    fn the_parameters_are_what_the_decision_says() {
        // Written down in docs/identity-design.md and asserted here, so
        // weakening them is a deliberate edit that fails the suite rather than
        // a quiet change nobody reviews.
        assert_eq!(KDF_MEMORY_KIB, 65_536);
        assert_eq!(KDF_ITERATIONS, 3);
        assert_eq!(KDF_PARALLELISM, 1);
    }

    #[test]
    fn one_derivation_is_costly_but_affordable_on_this_platform() {
        // "Measured on the slowest supported target" means running this suite
        // there, rather than writing a number in a document once.
        //
        // Both bounds matter, and the lower one matters more. A derivation
        // that got *faster* is the failure mode worth catching: it means the
        // parameters were weakened, and no user would ever notice. The upper
        // bound is deliberately loose because this runs alongside every other
        // test in the suite, each of which may be holding 64 MiB of its own —
        // it is here to catch a configuration nobody could live with, not to
        // measure the machine.
        let params = KdfParams::fresh().unwrap();

        let started = std::time::Instant::now();
        params.derive("correct horse battery staple", None).unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed > std::time::Duration::from_millis(20),
            "deriving the vault key took only {elapsed:?} — the parameters cannot be what they claim"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "deriving the vault key took {elapsed:?}, which is too slow to put in front of a user"
        );
    }

    #[test]
    fn the_key_file_never_contains_the_key() {
        // The entire point. Reading the envelope must not reveal what it wraps.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();

        let key = provider.data_key().unwrap();
        let raw = std::fs::read_to_string(dir.path().join("vault.key.enc")).unwrap();

        assert!(!raw.contains(&hex::encode(key.expose_for_storage())));
        assert!(!raw.contains("passphrase"));
    }

    #[test]
    fn the_key_is_unavailable_until_unlocked() {
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);

        assert_eq!(provider.state(), VaultState::Uninitialized);
        assert!(matches!(provider.data_key(), Err(VaultError::KeyUnavailable)));
    }

    #[test]
    fn the_same_passphrase_returns_the_same_key() {
        // A provider that produced a different key each time would make every
        // previously written vault unreadable.
        let dir = TempDir::new().unwrap();
        let (first, _a) = open_provider(&dir);
        first.unlock(&secret("passphrase")).unwrap();
        let before = first.data_key().unwrap();

        let (second, _b) = open_provider(&dir);
        assert_eq!(second.state(), VaultState::Locked);
        second.unlock(&secret("passphrase")).unwrap();

        assert_eq!(
            before.expose_for_storage(),
            second.data_key().unwrap().expose_for_storage()
        );
    }

    #[test]
    fn a_wrong_passphrase_is_refused_and_changes_nothing() {
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();
        let expected = provider.data_key().unwrap().expose_for_storage();
        let envelope = std::fs::read(dir.path().join("vault.key.enc")).unwrap();

        let (other, _other_unlock) = open_provider(&dir);
        assert_eq!(other.unlock(&secret("wrong")), Err(UnlockFailure::WrongSecret));

        // Never regenerated, never overwritten, never presented as a fresh
        // install — any of those would silently orphan the wallet.
        assert!(matches!(other.data_key(), Err(VaultError::KeyUnavailable)));
        assert_eq!(std::fs::read(dir.path().join("vault.key.enc")).unwrap(), envelope);

        other.clear_attempts();
        other.unlock(&secret("passphrase")).unwrap();
        assert_eq!(other.data_key().unwrap().expose_for_storage(), expected);
    }

    #[test]
    fn separate_locations_get_separate_keys() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        let (first, _x) = open_provider(&a);
        let (second, _y) = open_provider(&b);
        first.unlock(&secret("same passphrase")).unwrap();
        second.unlock(&secret("same passphrase")).unwrap();

        assert_ne!(
            first.data_key().unwrap().expose_for_storage(),
            second.data_key().unwrap().expose_for_storage(),
            "a shared passphrase must not mean a shared key"
        );
    }

    #[test]
    fn a_corrupt_envelope_is_not_a_wrong_passphrase() {
        // Telling someone to retype a passphrase that was right all along is
        // its own kind of data loss.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        std::fs::write(dir.path().join("vault.key.enc"), "not json").unwrap();

        assert_eq!(provider.unlock(&secret("passphrase")), Err(UnlockFailure::Unusable));
    }

    #[test]
    fn an_unknown_derivation_is_refused_rather_than_guessed() {
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();

        let path = dir.path().join("vault.key.enc");
        let mut envelope: KeyEnvelope =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        envelope.kdf.algorithm = "scrypt".into();
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();

        let (reopened, _again) = open_provider(&dir);
        assert_eq!(reopened.unlock(&secret("passphrase")), Err(UnlockFailure::Unusable));
    }

    #[test]
    fn tampering_with_the_wrapped_key_is_detected() {
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();

        let path = dir.path().join("vault.key.enc");
        let mut envelope: KeyEnvelope =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        envelope.wrapped[0] ^= 0xff;
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();

        let (reopened, _again) = open_provider(&dir);
        assert_eq!(reopened.unlock(&secret("passphrase")), Err(UnlockFailure::WrongSecret));
    }

    #[test]
    fn envelope_parameters_are_used_rather_than_the_constants() {
        // What makes raising the parameters later possible without orphaning
        // an existing vault.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();
        let expected = provider.data_key().unwrap().expose_for_storage();

        let path = dir.path().join("vault.key.enc");
        let envelope: KeyEnvelope =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(envelope.kdf.memory_kib, KDF_MEMORY_KIB);
        assert_eq!(envelope.kdf.iterations, KDF_ITERATIONS);

        let (reopened, _again) = open_provider(&dir);
        reopened.unlock(&secret("passphrase")).unwrap();
        assert_eq!(reopened.data_key().unwrap().expose_for_storage(), expected);
    }

    #[test]
    fn a_plaintext_key_migrates_and_is_removed() {
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("vault.key");
        let original = [7_u8; 32];
        std::fs::write(&legacy, hex::encode(original)).unwrap();

        let (provider, _unlock) = open_provider(&dir);
        assert_eq!(provider.state(), VaultState::Locked, "an existing vault must not look uninitialized");
        provider.unlock(&secret("passphrase")).unwrap();

        // The same key, so the vault written under it still opens.
        assert_eq!(provider.data_key().unwrap().expose_for_storage(), original);
        assert!(!legacy.exists(), "the plaintext key survived migration");
        assert!(dir.path().join("vault.key.enc").exists());
    }

    #[test]
    fn a_failed_migration_leaves_the_plaintext_intact() {
        // The dangerous path: if wrapping fails, the only copy of the key must
        // survive untouched rather than be deleted on optimism.
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("vault.key");
        std::fs::write(&legacy, hex::encode([7_u8; 32])).unwrap();

        // An unwritable envelope path: a directory where a file must go.
        std::fs::create_dir(dir.path().join("vault.key.enc")).unwrap();

        let (provider, _unlock) = open_provider(&dir);
        assert!(provider.unlock(&secret("passphrase")).is_err());
        assert!(legacy.exists(), "a failed migration destroyed the only key");
        assert_eq!(std::fs::read_to_string(&legacy).unwrap(), hex::encode([7_u8; 32]));
    }

    #[test]
    fn an_interrupted_migration_is_finished_on_the_next_unlock() {
        // Envelope written, plaintext not yet removed — the exact state a
        // crash between those two steps leaves behind.
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("vault.key");
        let original = [9_u8; 32];
        std::fs::write(&legacy, hex::encode(original)).unwrap();

        let (first, _a) = open_provider(&dir);
        first.unlock(&secret("passphrase")).unwrap();
        // Re-create the plaintext to simulate the interruption.
        std::fs::write(&legacy, hex::encode(original)).unwrap();

        let (second, _b) = open_provider(&dir);
        second.unlock(&secret("passphrase")).unwrap();

        assert_eq!(second.data_key().unwrap().expose_for_storage(), original);
        assert!(!legacy.exists(), "the redundant plaintext was left on disk");
    }

    #[test]
    fn a_conflicting_plaintext_key_is_kept_rather_than_deleted() {
        // Two different keys means two different vaults, and deleting either
        // could be deleting a wallet.
        let dir = TempDir::new().unwrap();
        let (first, _a) = open_provider(&dir);
        first.unlock(&secret("passphrase")).unwrap();

        let legacy = dir.path().join("vault.key");
        std::fs::write(&legacy, hex::encode([1_u8; 32])).unwrap();

        let (second, _b) = open_provider(&dir);
        second.unlock(&secret("passphrase")).unwrap();

        assert!(legacy.exists(), "a key that opens a different vault was destroyed");
    }

    #[test]
    fn repeated_failures_are_throttled_and_survive_a_restart() {
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();

        let (attacker, _theirs) = open_provider(&dir);
        for _ in 0..=FREE_ATTEMPTS {
            let _ = attacker.unlock(&secret("wrong"));
        }

        // A fresh provider — as after killing and reopening the app — still
        // sees the count, and refuses on that basis rather than on anything
        // held in memory by the process that recorded it.
        let (restarted, _restarted_unlock) = open_provider(&dir);
        assert_eq!(
            restarted.attempts().map(|record| record.failures),
            Some(FREE_ATTEMPTS + 1),
            "the attempt count reset when the process did"
        );
        assert!(
            matches!(
                restarted.unlock(&secret("wrong")),
                Err(UnlockFailure::RateLimited { .. })
            ),
            "an attempt was allowed while inside the backoff window"
        );
        // Even the correct passphrase waits: a refusal that could be skipped
        // by knowing the answer would not be a delay at all.
        assert!(matches!(
            restarted.unlock(&secret("passphrase")),
            Err(UnlockFailure::RateLimited { .. })
        ));
    }

    #[test]
    fn a_successful_unlock_clears_the_backoff() {
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();

        let (typo, _a) = open_provider(&dir);
        let _ = typo.unlock(&secret("wrong"));
        typo.unlock(&secret("passphrase")).unwrap();

        let (again, _b) = open_provider(&dir);
        assert!(again.unlock(&secret("wrong")).is_err());
        // Back to the free allowance rather than continuing the old streak.
        assert_eq!(again.attempts().unwrap().failures, 1);
    }

    #[test]
    fn locking_requires_the_passphrase_again() {
        let dir = TempDir::new().unwrap();
        let (provider, unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();
        assert!(provider.data_key().is_ok());

        unlock.lock();

        assert_eq!(provider.state(), VaultState::Locked);
        assert!(matches!(provider.data_key(), Err(VaultError::KeyUnavailable)));
    }

    use crate::device_binding::with_test_store as with_store;

    #[test]
    fn a_bound_key_does_not_open_on_a_machine_without_the_store() {
        // The demonstration the whole layer exists for: the file alone, and
        // even the file plus the passphrase, is not enough somewhere else.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        with_store(crate::device_binding::TestStore::ThisDevice, || {
            provider.unlock(&secret("passphrase")).unwrap();
        });

        let (elsewhere, _theirs) = open_provider(&dir);
        let outcome = with_store(crate::device_binding::TestStore::Unavailable, || {
            elsewhere.unlock(&secret("passphrase"))
        });

        assert_eq!(outcome, Err(UnlockFailure::DeviceBindingUnavailable));
        assert!(matches!(elsewhere.data_key(), Err(VaultError::KeyUnavailable)));
    }

    #[test]
    fn a_bound_key_does_not_open_on_a_different_device() {
        // The subtler half: the other machine *has* a store, it just holds a
        // different secret. Without the fingerprint this would surface as a
        // wrong passphrase and send someone looking for a typo.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        with_store(crate::device_binding::TestStore::ThisDevice, || {
            provider.unlock(&secret("passphrase")).unwrap();
        });

        let (elsewhere, _theirs) = open_provider(&dir);
        let outcome = with_store(crate::device_binding::TestStore::AnotherDevice, || {
            elsewhere.unlock(&secret("passphrase"))
        });

        assert_eq!(outcome, Err(UnlockFailure::DeviceBindingUnavailable));
    }

    #[test]
    fn a_refused_store_still_produces_a_working_vault() {
        // Degrades to passphrase-only rather than refusing to start, and says
        // so in the envelope rather than claiming a binding it does not have.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);

        with_store(crate::device_binding::TestStore::Unavailable, || {
            provider.unlock(&secret("passphrase")).unwrap();
        });
        assert!(provider.data_key().is_ok());

        let envelope: KeyEnvelope = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("vault.key.enc")).unwrap(),
        )
        .unwrap();
        assert_eq!(envelope.binding, "none");
        assert!(envelope.binding_check.is_empty());
    }

    #[test]
    fn an_unbound_key_opens_anywhere() {
        // The converse, so the refusals above are attributable to the binding
        // rather than to anything else the test moved.
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        with_store(crate::device_binding::TestStore::Unavailable, || {
            provider.unlock(&secret("passphrase")).unwrap();
        });

        let (elsewhere, _theirs) = open_provider(&dir);
        with_store(crate::device_binding::TestStore::AnotherDevice, || {
            elsewhere.unlock(&secret("passphrase")).unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn the_envelope_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let (provider, _unlock) = open_provider(&dir);
        provider.unlock(&secret("passphrase")).unwrap();

        let mode = std::fs::metadata(dir.path().join("vault.key.enc"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the envelope is readable by others");
    }
}
