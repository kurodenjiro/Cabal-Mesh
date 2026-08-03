//! The one place legacy and domain shapes are allowed to meet.
//!
//! Every conversion between the frozen IPC surface and the reshaped services
//! lives here, so the compatibility boundary is a single reviewable file
//! rather than fifty scattered casts spread through the command bodies.
//!
//! # Why it is nearly empty right now
//!
//! Ticket 11 lands this seam *before* the services move, while the legacy
//! commands still call the original code directly. That is deliberate: the
//! adapter is cheap to write as a pass-through and expensive to retrofit once
//! signatures have already changed underneath it.
//!
//! It fills in as later tickets land:
//!
//! - **Ticket 12** introduces the typed error union. [`flatten_error`] becomes
//!   the single point where it collapses back to the `String` the frozen UI
//!   expects.
//! - **Ticket 14** reshapes application state. Handle adaptation lands here.
//! - **Tickets 17–24** move storage, vault and chain. Their view types get
//!   `From` impls here rather than in command bodies.
//!
//! # Rules
//!
//! - Conversions are `From`/`TryFrom` impls where possible, so they are
//!   discoverable and testable rather than ad-hoc helper calls.
//! - Nothing here is used by the new command surface. If a conversion is
//!   useful to both, it belongs in the domain crate, not in the compatibility
//!   layer.
//! - The `String` error type is a frozen contract, not a style choice. Do not
//!   "improve" it.

/// Collapses an internal error into the flat string the frozen UI expects.
///
/// Deliberately lossy: the frozen frontend has no way to branch on a variant,
/// so it receives the display form. The full source chain is logged rather
/// than returned — RPC URLs, filesystem paths and key material must not travel
/// to the webview inside an error string.
///
/// Once ticket 12 introduces the typed error union, this is the only place it
/// is permitted to be flattened.
pub fn flatten_error<E: std::fmt::Display>(error: E) -> String {
    // Delegates to `crate::error::flatten`, which is unconditionally compiled.
    // A few frozen commands live in always-compiled modules and cannot reach
    // into this `cfg(desktop)`-gated one.
    crate::error::flatten(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_to_the_display_form() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "snapshot missing");
        assert_eq!(flatten_error(error), "snapshot missing");
    }
}
