//! The self-extracting executable target (`--emit=exe`).
//!
//! This is the historical `rakc build` output, now one target among several:
//! the rakc binary itself, with the program's source appended behind a small
//! footer. At startup such a binary recognises its own footer, extracts the
//! embedded source, and runs it on the interpreter — so the artifact is
//! standalone with no Rak installation on the target machine, at the cost of
//! depending on the rakc binary's own size.
//!
//! It is a *packaging* strategy, not compilation: the source is carried
//! verbatim rather than translated. For a compiled artifact, use
//! `--emit=bytecode`, `--emit=c`, or `--emit=link`.

use super::{Artifact, CodegenOptions, EmitKind, Target, Triple};

/// The footer magic. The last 8 bytes of a built executable; its presence is
/// what distinguishes a packaged rak program from an ordinary rakc binary.
pub const PAYLOAD_MAGIC: u64 = 0x52414B5F50434B; // "RAK_PCK"

/// Emits [`EmitKind::Exe`].
pub struct SelfExe;

impl Target for SelfExe {
    fn id(&self) -> &'static str {
        "exe"
    }

    fn supports(&self, kind: EmitKind, _triple: &Triple) -> bool {
        kind == EmitKind::Exe
    }

    fn emit(
        &self,
        _module: &crate::ast::Module,
        opts: &CodegenOptions,
        kind: EmitKind,
    ) -> Result<Artifact, String> {
        let source = std::fs::read_to_string(&opts.source_path)
            .map_err(|e| format!("Error reading source: {}", e))?;
        // The running rakc is the runtime; the artifact is a copy of it with a
        // payload. Reading this process's own executable is what makes that
        // work on both platforms without shipping a separate runtime.
        let exe_path =
            std::env::current_exe().map_err(|e| format!("locating the rakc binary: {}", e))?;
        let exe_data =
            std::fs::read(&exe_path).map_err(|e| format!("Error reading rakc binary: {}", e))?;

        let source_bytes = source.as_bytes();
        let mut out = Vec::with_capacity(exe_data.len() + source_bytes.len() + 16);
        out.extend_from_slice(&exe_data);
        out.extend_from_slice(source_bytes);
        out.extend_from_slice(&(source_bytes.len() as u64).to_be_bytes());
        out.extend_from_slice(&PAYLOAD_MAGIC.to_be_bytes());

        Ok(Artifact::File { kind, data: out })
    }
}

/// Whether `bytes` (a rakc binary or a copy of one) carries an embedded
/// payload, and if so the source it carries.
///
/// Checked at startup on every invocation, so it must be cheap: a length test
/// and an 8-byte comparison, with no scan of the whole binary.
pub fn embedded_source(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 16 {
        return None;
    }
    let magic_bytes = &bytes[bytes.len() - 8..];
    let magic = u64::from_be_bytes(magic_bytes.try_into().ok()?);
    if magic != PAYLOAD_MAGIC {
        return None;
    }
    let len_bytes = &bytes[bytes.len() - 16..bytes.len() - 8];
    let source_len = u64::from_be_bytes(len_bytes.try_into().ok()?) as usize;
    // The length must describe a payload that actually fits inside the file,
    // header included. `saturating` arithmetic keeps a corrupt length from
    // wrapping past the check below.
    let payload_start = match source_len.checked_add(16) {
        Some(total) if total <= bytes.len() => bytes.len() - total,
        _ => return None,
    };
    String::from_utf8(bytes[payload_start..bytes.len() - 16].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_source_roundtrips() {
        let program = "dump \"hello from a built binary\"\nfn main(argv) { return 0 }\n";
        let mut exe = b"a fake rakc binary with some size to it".to_vec();
        exe.extend_from_slice(program.as_bytes());
        exe.extend_from_slice(&(program.len() as u64).to_be_bytes());
        exe.extend_from_slice(&PAYLOAD_MAGIC.to_be_bytes());

        let back = embedded_source(&exe).expect("payload");
        assert_eq!(back, program);
    }

    #[test]
    fn an_ordinary_binary_has_no_payload() {
        let exe = b"just a regular rakc, nothing appended".to_vec();
        assert!(embedded_source(&exe).is_none());
    }

    #[test]
    fn a_short_file_is_not_a_payload() {
        assert!(embedded_source(b"MZ").is_none());
        assert!(embedded_source(&[]).is_none());
    }

    #[test]
    fn a_payload_length_past_the_file_is_rejected() {
        // A footer whose length claims more bytes than exist must not be
        // interpreted as one: `saturating_sub` clamps, and the range would be
        // nonsense, so the answer is "no payload" rather than a slice panic.
        let mut exe = b"tiny".to_vec();
        exe.extend_from_slice(&u64::MAX.to_be_bytes());
        exe.extend_from_slice(&PAYLOAD_MAGIC.to_be_bytes());
        assert!(embedded_source(&exe).is_none());
    }
}
