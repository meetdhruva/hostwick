//! GSS-API (RFC 2744 C bindings) loaded at run time with `libloading`:
//! MIT or Heimdal `libgssapi` on Linux/BSD, `GSS.framework` on macOS.
//!
//! Only the handful of calls `gssapi-with-mic` needs are bound.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::OnceLock;

use super::{GssFailure, SecurityContext, Token};

type OmUint32 = u32;
type GssName = *mut c_void;
type GssCtx = *mut c_void;
type GssCred = *mut c_void;

/// `gss_buffer_desc`.
#[repr(C)]
struct Buffer {
    length: usize,
    value: *mut c_void,
}

impl Buffer {
    fn empty() -> Self {
        Self {
            length: 0,
            value: std::ptr::null_mut(),
        }
    }

    /// Borrow `data` as an input buffer. The library never writes to it.
    fn borrowed(data: &[u8]) -> Self {
        Self {
            length: data.len(),
            value: data.as_ptr().cast_mut().cast(),
        }
    }
}

/// `gss_OID_desc`. Apple's headers pack it to 2 bytes on x86_64 (the
/// pointer sits at offset 4); everywhere else it has natural C layout.
#[cfg_attr(all(target_os = "macos", target_arch = "x86_64"), repr(C, packed(2)))]
#[cfg_attr(not(all(target_os = "macos", target_arch = "x86_64")), repr(C))]
struct Oid {
    length: OmUint32,
    elements: *mut c_void,
}

/// Raw OID bodies (no DER tag/length).
const KRB5_MECH: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x12, 0x01, 0x02, 0x02];
/// GSS_C_NT_HOSTBASED_SERVICE, 1.2.840.113554.1.2.1.4.
const NT_HOSTBASED_SERVICE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x12, 0x01, 0x02, 0x01, 0x04];

fn oid(body: &'static [u8]) -> Oid {
    Oid {
        length: body.len() as OmUint32,
        elements: body.as_ptr().cast_mut().cast(),
    }
}

const GSS_C_DELEG_FLAG: OmUint32 = 1;
const GSS_C_MUTUAL_FLAG: OmUint32 = 2;
const GSS_C_INTEG_FLAG: OmUint32 = 32;
const GSS_C_GSS_CODE: i32 = 1;
const GSS_C_MECH_CODE: i32 = 2;
#[cfg(test)]
const GSS_S_COMPLETE: OmUint32 = 0;
const GSS_S_CONTINUE_NEEDED: OmUint32 = 1;
const GSS_S_BAD_NAME: OmUint32 = 2 << 16;
const GSS_S_NO_CRED: OmUint32 = 7 << 16;
const GSS_S_CREDENTIALS_EXPIRED: OmUint32 = 11 << 16;
const ROUTINE_ERROR_MASK: OmUint32 = 0xff << 16;
const CALLING_ERROR_MASK: OmUint32 = 0xff << 24;

fn is_error(major: OmUint32) -> bool {
    major & (ROUTINE_ERROR_MASK | CALLING_ERROR_MASK) != 0
}

type ImportNameFn =
    unsafe extern "C" fn(*mut OmUint32, *mut Buffer, *mut Oid, *mut GssName) -> OmUint32;
type InitSecContextFn = unsafe extern "C" fn(
    *mut OmUint32, // minor_status
    GssCred,       // claimant_cred_handle
    *mut GssCtx,   // context_handle
    GssName,       // target_name
    *mut Oid,      // mech_type
    OmUint32,      // req_flags
    OmUint32,      // time_req
    *mut c_void,   // input_chan_bindings
    *mut Buffer,   // input_token
    *mut *mut Oid, // actual_mech_type
    *mut Buffer,   // output_token
    *mut OmUint32, // ret_flags
    *mut OmUint32, // time_rec
) -> OmUint32;
type GetMicFn =
    unsafe extern "C" fn(*mut OmUint32, GssCtx, OmUint32, *mut Buffer, *mut Buffer) -> OmUint32;
type ReleaseBufferFn = unsafe extern "C" fn(*mut OmUint32, *mut Buffer) -> OmUint32;
type ReleaseNameFn = unsafe extern "C" fn(*mut OmUint32, *mut GssName) -> OmUint32;
type DeleteSecContextFn = unsafe extern "C" fn(*mut OmUint32, *mut GssCtx, *mut Buffer) -> OmUint32;
type DisplayStatusFn = unsafe extern "C" fn(
    *mut OmUint32,
    OmUint32,
    i32,
    *mut Oid,
    *mut OmUint32,
    *mut Buffer,
) -> OmUint32;

/// The loaded library and the functions we use. Lives for the whole
/// process (in a `static`), so the function pointers stay valid.
struct Gss {
    _lib: libloading::Library,
    import_name: ImportNameFn,
    init_sec_context: InitSecContextFn,
    get_mic: GetMicFn,
    release_buffer: ReleaseBufferFn,
    release_name: ReleaseNameFn,
    delete_sec_context: DeleteSecContextFn,
    display_status: DisplayStatusFn,
}

fn candidates() -> Vec<String> {
    let mut v = Vec::new();
    // An explicit library (unusual installs, or testing) replaces the search.
    if let Ok(p) = std::env::var("HOSTWICK_GSSAPI_LIB")
        && !p.is_empty()
    {
        v.push(p);
        return v;
    }
    if cfg!(target_os = "macos") {
        v.push("/System/Library/Frameworks/GSS.framework/GSS".into());
        v.push("libgssapi_krb5.dylib".into());
    } else {
        // MIT (Debian/Ubuntu/Fedora), then Heimdal (BSDs, some distros).
        for n in [
            "libgssapi_krb5.so.2",
            "libgssapi_krb5.so",
            "libgssapi.so.3",
            "libgssapi.so",
        ] {
            v.push(n.into());
        }
    }
    v
}

fn library() -> Result<&'static Gss, GssFailure> {
    static LIB: OnceLock<Result<Gss, String>> = OnceLock::new();
    LIB.get_or_init(load)
        .as_ref()
        .map_err(|e| GssFailure(e.clone()))
}

fn load() -> Result<Gss, String> {
    let mut errors = Vec::new();
    for name in candidates() {
        // SAFETY: loading a system GSS-API library runs its initializers;
        // these are the platform's own Kerberos libraries, loaded by name
        // from the system search path (or an explicit override).
        let lib = match unsafe { libloading::Library::new(&name) } {
            Ok(l) => l,
            Err(e) => {
                errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        match bind(lib) {
            Ok(g) => {
                tracing::debug!(library = %name, "loaded GSS-API");
                return Ok(g);
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    tracing::debug!(?errors, "no GSS-API library");
    if std::env::var_os("HOSTWICK_GSSAPI_LIB").is_some_and(|v| !v.is_empty()) {
        return Err(format!(
            "cannot load the Kerberos library from HOSTWICK_GSSAPI_LIB ({})",
            errors.join("; ")
        ));
    }
    Err(if cfg!(target_os = "macos") {
        "the system GSS framework could not be loaded".into()
    } else {
        "no Kerberos (GSS-API) library found; install MIT Kerberos \
         (Debian/Ubuntu: libgssapi-krb5-2 krb5-user, Fedora: krb5-libs krb5-workstation)"
            .into()
    })
}

fn bind(lib: libloading::Library) -> Result<Gss, libloading::Error> {
    // SAFETY: each symbol is looked up by its RFC 2744 name and given the
    // RFC 2744 C signature (`OM_uint32` = u32, handles are pointers,
    // `gss_buffer_t`/`gss_OID` point to the #[repr(C)] structs above). The
    // copied function pointers are only used while `lib` is alive: it is
    // stored next to them in a process-wide static and never unloaded.
    unsafe {
        let import_name = *lib.get::<ImportNameFn>(b"gss_import_name\0")?;
        let init_sec_context = *lib.get::<InitSecContextFn>(b"gss_init_sec_context\0")?;
        let get_mic = *lib.get::<GetMicFn>(b"gss_get_mic\0")?;
        let release_buffer = *lib.get::<ReleaseBufferFn>(b"gss_release_buffer\0")?;
        let release_name = *lib.get::<ReleaseNameFn>(b"gss_release_name\0")?;
        let delete_sec_context = *lib.get::<DeleteSecContextFn>(b"gss_delete_sec_context\0")?;
        let display_status = *lib.get::<DisplayStatusFn>(b"gss_display_status\0")?;
        Ok(Gss {
            _lib: lib,
            import_name,
            init_sec_context,
            get_mic,
            release_buffer,
            release_name,
            delete_sec_context,
            display_status,
        })
    }
}

impl Gss {
    /// Copy a library-owned buffer into a Vec and release it.
    fn take_buffer(&self, buf: &mut Buffer) -> Vec<u8> {
        let out = if buf.value.is_null() || buf.length == 0 {
            Vec::new()
        } else {
            // SAFETY: the library filled `buf` with `length` readable bytes
            // at `value`; we copy them before releasing the buffer.
            unsafe { std::slice::from_raw_parts(buf.value.cast::<u8>(), buf.length).to_vec() }
        };
        let mut minor = 0;
        // SAFETY: `buf` was produced by this library (or is empty, which
        // gss_release_buffer accepts).
        unsafe { (self.release_buffer)(&mut minor, buf) };
        out
    }

    /// All messages for a status code (`gss_display_status` loop).
    fn messages(&self, code: OmUint32, kind: i32) -> Vec<String> {
        let mut out = Vec::new();
        let mut msg_ctx: OmUint32 = 0;
        let mut mech = oid(KRB5_MECH);
        for _ in 0..8 {
            let mut minor = 0;
            let mut buf = Buffer::empty();
            let mech_ptr: *mut Oid = if kind == GSS_C_MECH_CODE {
                &mut mech
            } else {
                std::ptr::null_mut()
            };
            // SAFETY: valid out-pointers to locals; `mech_ptr` is null or
            // points to a live OID whose elements are a static slice.
            let major = unsafe {
                (self.display_status)(&mut minor, code, kind, mech_ptr, &mut msg_ctx, &mut buf)
            };
            let text = self.take_buffer(&mut buf);
            if is_error(major) {
                break;
            }
            let text = String::from_utf8_lossy(&text).trim().to_owned();
            if !text.is_empty() {
                out.push(text);
            }
            if msg_ctx == 0 {
                break;
            }
        }
        out
    }

    fn failure(&self, what: &str, host: &str, major: OmUint32, minor: OmUint32) -> GssFailure {
        let routine = major & ROUTINE_ERROR_MASK;
        let mech_msgs = if minor != 0 {
            self.messages(minor, GSS_C_MECH_CODE)
        } else {
            Vec::new()
        };
        // "Unspecified GSS failure. Minor code may provide more information"
        // adds nothing when the mechanism explained itself.
        let gss_msgs: Vec<String> = self
            .messages(major, GSS_C_GSS_CODE)
            .into_iter()
            .filter(|m| mech_msgs.is_empty() || !m.starts_with("Unspecified GSS failure"))
            .collect();
        let detail = mech_msgs
            .iter()
            .chain(gss_msgs.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        tracing::debug!(what, major, minor, %detail, "GSS-API failure");
        GssFailure(classify(routine, &detail, host).unwrap_or_else(|| {
            if detail.is_empty() {
                format!("{what} failed (GSS major 0x{major:08x}, minor {minor})")
            } else {
                format!("{what} failed: {detail}")
            }
        }))
    }
}

/// Friendly messages for the common cases.
fn classify(routine: OmUint32, detail: &str, host: &str) -> Option<String> {
    let d = detail.to_ascii_lowercase();
    if routine == GSS_S_CREDENTIALS_EXPIRED
        || d.contains("ticket expired")
        || d.contains("credentials have expired")
    {
        return Some("Kerberos ticket expired — run kinit".into());
    }
    if routine == GSS_S_NO_CRED
        || d.contains("no kerberos credentials")
        || d.contains("no credentials cache")
        || d.contains("credentials cache file") && d.contains("not found")
    {
        return Some("no Kerberos ticket — run kinit".into());
    }
    if d.contains("not found in kerberos database") || routine == GSS_S_BAD_NAME {
        return Some(format!(
            "no Kerberos principal for host/{host} ({detail}); check the host name \
             (Kerberos needs the name the server's principal was created for)"
        ));
    }
    if d.contains("cannot find kdc") || d.contains("cannot contact any kdc") {
        return Some(format!("cannot reach the KDC: {detail}"));
    }
    None
}

pub(crate) struct Context {
    gss: &'static Gss,
    ctx: GssCtx,
    name: GssName,
    flags: OmUint32,
    host: String,
}

// SAFETY: a GSS security context and name are plain heap objects owned by
// this struct; they are only used through `&mut self` (never concurrently)
// and GSS-API implementations do not tie them to the creating thread.
unsafe impl Send for Context {}

impl Context {
    fn init(&mut self, input: Option<&[u8]>) -> Result<Token, GssFailure> {
        let mut minor = 0;
        let mut mech = oid(KRB5_MECH);
        let mut input_buf = input.map(Buffer::borrowed);
        let input_ptr: *mut Buffer = match input_buf.as_mut() {
            Some(b) => b,
            None => std::ptr::null_mut(),
        };
        let mut out = Buffer::empty();
        let mut ret_flags: OmUint32 = 0;
        // SAFETY: all pointers are to live locals or fields; `self.ctx` is
        // null on the first call (GSS_C_NO_CONTEXT) and afterwards the
        // handle the library returned; `self.name` came from
        // gss_import_name; the input buffer borrows `input` for the call.
        let major = unsafe {
            (self.gss.init_sec_context)(
                &mut minor,
                std::ptr::null_mut(), // GSS_C_NO_CREDENTIAL: default ccache
                &mut self.ctx,
                self.name,
                &mut mech,
                self.flags,
                0,
                std::ptr::null_mut(), // no channel bindings
                input_ptr,
                std::ptr::null_mut(),
                &mut out,
                &mut ret_flags,
                std::ptr::null_mut(),
            )
        };
        let token = self.gss.take_buffer(&mut out);
        if is_error(major) {
            return Err(self
                .gss
                .failure("Kerberos context setup", &self.host, major, minor));
        }
        let complete = major & GSS_S_CONTINUE_NEEDED == 0;
        Ok(Token {
            token,
            complete,
            integrity: ret_flags & GSS_C_INTEG_FLAG != 0,
        })
    }
}

impl SecurityContext for Context {
    fn initiate(host: &str, delegate: bool) -> Result<(Self, Token), GssFailure> {
        let gss = library()?;
        let service = format!("host@{host}");
        let mut name_buf = Buffer::borrowed(service.as_bytes());
        let mut name_type = oid(NT_HOSTBASED_SERVICE);
        let mut name: GssName = std::ptr::null_mut();
        let mut minor = 0;
        // SAFETY: input buffer borrows `service` for the call; the OID
        // points to a static body; `name` receives a library-owned handle
        // that `Context::drop` releases.
        let major =
            unsafe { (gss.import_name)(&mut minor, &mut name_buf, &mut name_type, &mut name) };
        if is_error(major) {
            return Err(gss.failure("importing the server name", host, major, minor));
        }
        let mut ctx = Context {
            gss,
            ctx: std::ptr::null_mut(),
            name,
            flags: GSS_C_MUTUAL_FLAG
                | GSS_C_INTEG_FLAG
                | if delegate { GSS_C_DELEG_FLAG } else { 0 },
            host: host.to_owned(),
        };
        let first = ctx.init(None)?;
        Ok((ctx, first))
    }

    fn step(&mut self, input: &[u8]) -> Result<Token, GssFailure> {
        self.init(Some(input))
    }

    fn mic(&mut self, data: &[u8]) -> Result<Vec<u8>, GssFailure> {
        let mut minor = 0;
        let mut msg = Buffer::borrowed(data);
        let mut out = Buffer::empty();
        // SAFETY: `self.ctx` is an established context from
        // gss_init_sec_context; `msg` borrows `data` for the call; `out`
        // receives a library buffer that take_buffer releases.
        let major = unsafe { (self.gss.get_mic)(&mut minor, self.ctx, 0, &mut msg, &mut out) };
        let mic = self.gss.take_buffer(&mut out);
        if is_error(major) {
            return Err(self.gss.failure("signing (MIC)", &self.host, major, minor));
        }
        Ok(mic)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let mut minor = 0;
        // SAFETY: both handles are null or were returned by this library
        // and are released exactly once here; the functions accept and
        // reset null/owned handles.
        unsafe {
            if !self.ctx.is_null() {
                (self.gss.delete_sec_context)(&mut minor, &mut self.ctx, std::ptr::null_mut());
            }
            if !self.name.is_null() {
                (self.gss.release_name)(&mut minor, &mut self.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_failures() {
        assert_eq!(
            classify(GSS_S_NO_CRED, "", "h").as_deref(),
            Some("no Kerberos ticket — run kinit")
        );
        assert_eq!(
            classify(
                13 << 16,
                "No Kerberos credentials available (default cache: FILE:/tmp/krb5cc_0)",
                "h"
            )
            .as_deref(),
            Some("no Kerberos ticket — run kinit")
        );
        assert!(classify(13 << 16, "Ticket expired", "h").is_some_and(|m| m.contains("expired")));
        assert!(
            classify(
                13 << 16,
                "Server host/db1@EXAMPLE.COM not found in Kerberos database",
                "db1"
            )
            .is_some_and(|m| m.contains("host/db1"))
        );
        assert!(classify(13 << 16, "something else", "h").is_none());
    }

    #[test]
    fn status_bits() {
        assert!(!is_error(GSS_S_COMPLETE));
        assert!(!is_error(GSS_S_CONTINUE_NEEDED));
        assert!(is_error(GSS_S_NO_CRED));
        assert!(is_error(1 << 24));
    }
}
