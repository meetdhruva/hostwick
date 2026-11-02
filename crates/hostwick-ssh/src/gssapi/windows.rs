//! Kerberos through Windows SSPI: the signed-in domain account's tickets,
//! no `kinit` needed. Same approach as PuTTY: the "Kerberos" package with
//! the `host/<hostname>` SPN, `MakeSignature` for the MIC.
#![allow(unsafe_code)]

use std::ffi::c_void;

use windows_sys::Win32::Security::Authentication::Identity::{
    AcquireCredentialsHandleW, DeleteSecurityContext, FreeContextBuffer, FreeCredentialsHandle,
    ISC_REQ_ALLOCATE_MEMORY, ISC_REQ_DELEGATE, ISC_REQ_INTEGRITY, ISC_REQ_MUTUAL_AUTH,
    ISC_RET_INTEGRITY, InitializeSecurityContextW, MakeSignature, QueryContextAttributesW,
    SECBUFFER_DATA, SECBUFFER_TOKEN, SECBUFFER_VERSION, SECPKG_ATTR_SIZES, SECPKG_CRED_OUTBOUND,
    SECURITY_NATIVE_DREP, SecBuffer, SecBufferDesc, SecPkgContext_Sizes,
};
use windows_sys::Win32::Security::Credentials::SecHandle;

use super::{GssFailure, SecurityContext, Token};

const SEC_E_OK: i32 = 0;
const SEC_I_CONTINUE_NEEDED: i32 = 0x0009_0312;
const SEC_I_COMPLETE_NEEDED: i32 = 0x0009_0313;
const SEC_I_COMPLETE_AND_CONTINUE: i32 = 0x0009_0314;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn describe(status: i32, host: &str) -> String {
    // Bit patterns of the SEC_E_* HRESULTs.
    match status as u32 {
        0x8009_030E => "no Kerberos credentials for this Windows sign-in \
                        (Kerberos needs a domain account; local accounts have none)"
            .into(),
        0x8009_0303 => {
            format!("the domain does not know host/{host} (no SPN registered for this server name)")
        }
        0x8009_0311 => "no domain controller could be reached".into(),
        0x8009_0322 => format!("the server is not host/{host} (wrong principal)"),
        0x8009_0324 => "clock skew too large between this computer and the domain".into(),
        0x8009_030C => "the domain refused the logon".into(),
        0x8009_0305 => "the Kerberos security package is not available".into(),
        other => format!("SSPI error 0x{other:08x}"),
    }
}

pub(crate) struct Context {
    cred: SecHandle,
    ctx: Option<SecHandle>,
    spn: Vec<u16>,
    flags: u32,
    host: String,
}

/// Copy an SSPI-allocated token and free it.
fn take(buf: &SecBuffer) -> Vec<u8> {
    if buf.pvBuffer.is_null() {
        return Vec::new();
    }
    // SAFETY: with ISC_REQ_ALLOCATE_MEMORY SSPI allocated `cbBuffer` bytes
    // at `pvBuffer`; we copy them and free the allocation once.
    unsafe {
        let v =
            std::slice::from_raw_parts(buf.pvBuffer.cast::<u8>(), buf.cbBuffer as usize).to_vec();
        FreeContextBuffer(buf.pvBuffer);
        v
    }
}

impl Context {
    fn init(&mut self, input: Option<&[u8]>) -> Result<Token, GssFailure> {
        let mut in_buf = SecBuffer {
            cbBuffer: input.map_or(0, |i| i.len() as u32),
            BufferType: SECBUFFER_TOKEN,
            pvBuffer: input.map_or(std::ptr::null_mut(), |i| i.as_ptr().cast_mut().cast()),
        };
        let in_desc = SecBufferDesc {
            ulVersion: SECBUFFER_VERSION,
            cBuffers: 1,
            pBuffers: &mut in_buf,
        };
        let mut out_buf = SecBuffer {
            cbBuffer: 0,
            BufferType: SECBUFFER_TOKEN,
            pvBuffer: std::ptr::null_mut(),
        };
        let mut out_desc = SecBufferDesc {
            ulVersion: SECBUFFER_VERSION,
            cBuffers: 1,
            pBuffers: &mut out_buf,
        };
        // Later calls pass the same handle in and out (as PuTTY does); the
        // first creates it.
        let mut new_ctx = self.ctx.unwrap_or_default();
        let mut attrs: u32 = 0;
        let mut expiry: i64 = 0;
        let ctx_ptr: *const SecHandle = match &self.ctx {
            Some(c) => c,
            None => std::ptr::null(),
        };
        let in_ptr: *const SecBufferDesc = if input.is_some() {
            &in_desc
        } else {
            std::ptr::null()
        };
        // SAFETY: `cred` is a live credential handle; `ctx_ptr` is null on
        // the first call or our live context; the SPN is NUL-terminated;
        // descriptors point to locals that outlive the call; the input
        // buffer borrows `input` read-only.
        let status = unsafe {
            InitializeSecurityContextW(
                &self.cred,
                ctx_ptr,
                self.spn.as_ptr(),
                self.flags,
                0,
                SECURITY_NATIVE_DREP,
                in_ptr,
                0,
                &mut new_ctx,
                &mut out_desc,
                &mut attrs,
                &mut expiry,
            )
        };
        let token = take(&out_buf);
        if status < 0 {
            return Err(GssFailure(describe(status, &self.host)));
        }
        self.ctx = Some(new_ctx);
        let complete = match status {
            SEC_E_OK => true,
            SEC_I_CONTINUE_NEEDED => false,
            SEC_I_COMPLETE_NEEDED | SEC_I_COMPLETE_AND_CONTINUE => {
                // Kerberos never asks for CompleteAuthToken.
                return Err(GssFailure(
                    "unexpected SSPI state (CompleteAuthToken requested)".into(),
                ));
            }
            other => {
                return Err(GssFailure(format!(
                    "unexpected SSPI status 0x{:08x}",
                    other as u32
                )));
            }
        };
        Ok(Token {
            token,
            complete,
            integrity: attrs & ISC_RET_INTEGRITY != 0,
        })
    }
}

impl SecurityContext for Context {
    fn initiate(host: &str, delegate: bool) -> Result<(Self, Token), GssFailure> {
        let package = wide("Kerberos");
        let mut cred = SecHandle::default();
        let mut expiry: i64 = 0;
        // SAFETY: package name is NUL-terminated and outlives the call; all
        // optional arguments are null; `cred` receives a handle that
        // `Drop` frees.
        let status = unsafe {
            AcquireCredentialsHandleW(
                std::ptr::null(),
                package.as_ptr(),
                SECPKG_CRED_OUTBOUND,
                std::ptr::null(),
                std::ptr::null(),
                None,
                std::ptr::null(),
                &mut cred,
                &mut expiry,
            )
        };
        if status != SEC_E_OK {
            return Err(GssFailure(describe(status, host)));
        }
        let mut ctx = Context {
            cred,
            ctx: None,
            spn: wide(&format!("host/{host}")),
            flags: ISC_REQ_MUTUAL_AUTH
                | ISC_REQ_INTEGRITY
                | ISC_REQ_ALLOCATE_MEMORY
                | if delegate { ISC_REQ_DELEGATE } else { 0 },
            host: host.to_owned(),
        };
        let first = ctx.init(None)?;
        Ok((ctx, first))
    }

    fn step(&mut self, input: &[u8]) -> Result<Token, GssFailure> {
        self.init(Some(input))
    }

    fn mic(&mut self, data: &[u8]) -> Result<Vec<u8>, GssFailure> {
        let Some(ctx) = &self.ctx else {
            return Err(GssFailure("no security context".into()));
        };
        let mut sizes = SecPkgContext_Sizes::default();
        // SAFETY: `ctx` is an established context; `sizes` is the struct
        // SECPKG_ATTR_SIZES fills.
        let status = unsafe {
            QueryContextAttributesW(
                ctx,
                SECPKG_ATTR_SIZES,
                (&mut sizes as *mut SecPkgContext_Sizes).cast::<c_void>(),
            )
        };
        if status != SEC_E_OK {
            return Err(GssFailure(describe(status, &self.host)));
        }
        let mut msg = data.to_vec();
        let mut sig = vec![0u8; sizes.cbMaxSignature as usize];
        let mut bufs = [
            SecBuffer {
                cbBuffer: msg.len() as u32,
                BufferType: SECBUFFER_DATA,
                pvBuffer: msg.as_mut_ptr().cast(),
            },
            SecBuffer {
                cbBuffer: sig.len() as u32,
                BufferType: SECBUFFER_TOKEN,
                pvBuffer: sig.as_mut_ptr().cast(),
            },
        ];
        let desc = SecBufferDesc {
            ulVersion: SECBUFFER_VERSION,
            cBuffers: 2,
            pBuffers: bufs.as_mut_ptr(),
        };
        // SAFETY: the descriptor points to two buffers backed by `msg` and
        // `sig`, both alive and sized as declared; SSPI writes at most
        // cbMaxSignature bytes into `sig` and updates its cbBuffer.
        let status = unsafe { MakeSignature(ctx, 0, &desc, 0) };
        if status != SEC_E_OK {
            return Err(GssFailure(describe(status, &self.host)));
        }
        sig.truncate(bufs[1].cbBuffer as usize);
        Ok(sig)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: handles were returned by SSPI and are released once.
        unsafe {
            if let Some(c) = &self.ctx {
                DeleteSecurityContext(c);
            }
            FreeCredentialsHandle(&self.cred);
        }
    }
}
