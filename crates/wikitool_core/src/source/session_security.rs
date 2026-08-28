use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

pub(super) fn write_private_session_file(path: &Path, content: &[u8]) -> Result<()> {
    let target = private_destination_path(path)?;
    let parent = target
        .parent()
        .ok_or_else(|| anyhow::anyhow!("source-session path has no parent"))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| "failed to create a private source-session staging file")?;

    // Establish and verify privacy before secret bytes enter the file. On Windows this is an
    // explicit protected DACL; on Unix it is mode 0600 plus current effective-user ownership.
    secure_session_file(temporary.path())
        .context("failed to secure a source-session staging file")?;
    temporary
        .write_all(content)
        .context("failed to stage source-session bytes")?;
    temporary
        .as_file()
        .sync_all()
        .context("failed to flush staged source-session bytes")?;
    temporary
        .persist(&target)
        .map_err(|error| error.error)
        .context("failed to atomically publish a source-session file")?;

    verify_published_session_file_with(&target, secure_session_file)
}

pub(super) fn remove_session_file(path: &Path) -> Result<bool> {
    let target = private_destination_path(path)?;
    match fs::remove_file(&target) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("failed to remove a source-session file"),
    }
}

fn verify_published_session_file_with<F>(path: &Path, verify: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if let Err(error) = verify(path) {
        let cleanup = match remove_session_file(path) {
            Ok(true) => "the rejected file was removed".to_string(),
            Ok(false) => "no rejected file remained".to_string(),
            Err(cleanup_error) => {
                format!("removing the rejected file also failed: {cleanup_error}")
            }
        };
        return Err(error).with_context(|| {
            format!("published source-session privacy verification failed; {cleanup}")
        });
    }
    Ok(())
}

#[cfg(not(windows))]
fn private_destination_path(path: &Path) -> Result<PathBuf> {
    Ok(path.to_path_buf())
}

#[cfg(windows)]
fn private_destination_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("source-session path has no parent"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("source-session path has no file name"))?;
    let mut target = parent
        .canonicalize()
        .context("failed to resolve the source-session directory")?;
    target.push(file_name);
    Ok(target)
}

#[cfg(unix)]
pub(super) fn secure_session_directory(path: &Path) -> Result<()> {
    unix::apply_and_verify_mode(path, 0o700)
}

#[cfg(unix)]
pub(super) fn secure_session_file(path: &Path) -> Result<()> {
    unix::apply_and_verify_mode(path, 0o600)
}

#[cfg(not(any(unix, windows)))]
pub(super) fn secure_session_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn secure_session_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
pub(super) fn secure_session_directory(path: &Path) -> Result<()> {
    windows::apply_and_verify_current_user_acl(path, true)
}

#[cfg(windows)]
pub(super) fn secure_session_file(path: &Path) -> Result<()> {
    windows::apply_and_verify_current_user_acl(path, false)
}

#[cfg(all(test, windows))]
pub(super) fn verify_session_directory(path: &Path) -> Result<()> {
    windows::verify_current_user_acl(path, true)
}

#[cfg(all(test, windows))]
pub(super) fn verify_session_file(path: &Path) -> Result<()> {
    windows::verify_current_user_acl(path, false)
}

#[cfg(all(test, unix))]
pub(super) fn verify_session_directory(path: &Path) -> Result<()> {
    unix::verify_mode(path, 0o700)
}

#[cfg(all(test, unix))]
pub(super) fn verify_session_file(path: &Path) -> Result<()> {
    unix::verify_mode(path, 0o600)
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    use anyhow::{Context, Result, bail};

    pub(super) fn apply_and_verify_mode(path: &Path, expected_mode: u32) -> Result<()> {
        fs::set_permissions(path, fs::Permissions::from_mode(expected_mode))
            .context("failed to apply private source-session permissions")?;
        verify_mode(path, expected_mode)
    }

    pub(super) fn verify_mode(path: &Path, expected_mode: u32) -> Result<()> {
        let metadata = fs::symlink_metadata(path)
            .context("failed to inspect private source-session permissions")?;
        let actual_mode = metadata.mode() & 0o7777;
        if actual_mode != expected_mode {
            bail!(
                "source-session permissions are {actual_mode:#06o}; expected {expected_mode:#06o}"
            );
        }
        let effective_user = unsafe { libc::geteuid() };
        if metadata.uid() != effective_user {
            bail!("source-session storage is not owned by the current effective user");
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::io;
    use std::mem::{align_of, size_of};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    use anyhow::{Context, Result, bail};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx, DACL_SECURITY_INFORMATION,
        EqualSid, GetAce, GetFileSecurityW, GetLengthSid, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, GetTokenInformation, InitializeAcl,
        IsValidSid, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
        SE_DACL_PROTECTED, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // Winnt.h defines ACCESS_ALLOWED_ACE_TYPE as zero. windows-sys places that constant behind
    // the much broader SystemServices feature, so keep the narrow ABI value local.
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

    pub(super) fn apply_and_verify_current_user_acl(path: &Path, directory: bool) -> Result<()> {
        let wide_path = wide_path(path)?;
        let user = CurrentUserSid::load()?;
        let sid = user.sid();
        let inheritance = inheritance_flags(directory);
        let sid_length = unsafe { GetLengthSid(sid) };
        if sid_length == 0 {
            return Err(io::Error::last_os_error()).context("failed to size the current-user SID");
        }

        // The variable-length SID replaces ACCESS_ALLOWED_ACE::SidStart.
        let acl_length = size_of::<ACL>()
            .checked_add(size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>())
            .and_then(|value| value.checked_add(sid_length as usize))
            .ok_or_else(|| anyhow::anyhow!("current-user ACL size overflow"))?;
        let acl_length_u32 = u32::try_from(acl_length).context("current-user ACL is too large")?;
        let mut acl_storage = AlignedBuffer::new(acl_length);
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();

        if unsafe { InitializeAcl(acl, acl_length_u32, ACL_REVISION) } == 0 {
            return Err(io::Error::last_os_error()).context("failed to initialize a private ACL");
        }
        if unsafe { AddAccessAllowedAceEx(acl, ACL_REVISION, inheritance, FILE_ALL_ACCESS, sid) }
            == 0
        {
            return Err(io::Error::last_os_error())
                .context("failed to add the current user to a private ACL");
        }

        let status = unsafe {
            SetNamedSecurityInfoW(
                wide_path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32))
                .context("failed to apply a protected current-user DACL");
        }

        verify_current_user_acl_with_sid(&wide_path, sid, directory)
    }

    #[cfg(test)]
    pub(super) fn verify_current_user_acl(path: &Path, directory: bool) -> Result<()> {
        let wide_path = wide_path(path)?;
        let user = CurrentUserSid::load()?;
        verify_current_user_acl_with_sid(&wide_path, user.sid(), directory)
    }

    fn verify_current_user_acl_with_sid(
        wide_path: &[u16],
        current_user_sid: PSID,
        directory: bool,
    ) -> Result<()> {
        let requested_information = DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION;
        let mut needed = 0_u32;
        unsafe {
            GetFileSecurityW(
                wide_path.as_ptr(),
                requested_information,
                ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 {
            return Err(io::Error::last_os_error())
                .context("failed to size a source-session security descriptor");
        }

        let mut descriptor = AlignedBuffer::new(needed as usize);
        if unsafe {
            GetFileSecurityW(
                wide_path.as_ptr(),
                requested_information,
                descriptor.as_mut_ptr(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error())
                .context("failed to read a source-session security descriptor");
        }

        let mut control = 0_u16;
        let mut revision = 0_u32;
        if unsafe {
            GetSecurityDescriptorControl(descriptor.as_mut_ptr(), &mut control, &mut revision)
        } == 0
        {
            return Err(io::Error::last_os_error())
                .context("failed to inspect source-session DACL protection");
        }
        if control & SE_DACL_PROTECTED == 0 {
            bail!("source-session DACL still inherits permissions");
        }

        let mut owner: PSID = ptr::null_mut();
        let mut owner_defaulted = 0;
        if unsafe {
            GetSecurityDescriptorOwner(descriptor.as_mut_ptr(), &mut owner, &mut owner_defaulted)
        } == 0
        {
            return Err(io::Error::last_os_error())
                .context("failed to inspect the source-session owner");
        }
        if owner.is_null() || unsafe { IsValidSid(owner) } == 0 {
            bail!("source-session storage has no valid owner SID");
        }
        if owner_defaulted != 0 {
            bail!("source-session storage has a defaulted owner SID");
        }
        if unsafe { EqualSid(owner, current_user_sid) } == 0 {
            bail!("source-session storage is not owned by the current user");
        }

        let mut present = 0;
        let mut defaulted = 0;
        let mut acl: *mut ACL = ptr::null_mut();
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor.as_mut_ptr(),
                &mut present,
                &mut acl,
                &mut defaulted,
            )
        } == 0
        {
            return Err(io::Error::last_os_error())
                .context("failed to inspect the source-session DACL");
        }
        if present == 0 || acl.is_null() {
            bail!("source-session storage has no explicit DACL");
        }
        if defaulted != 0 {
            bail!("source-session storage has a defaulted DACL");
        }

        let ace_count = unsafe { (*acl).AceCount };
        if ace_count != 1 {
            bail!(
                "source-session DACL contains {ace_count} entries; expected exactly one current-user entry"
            );
        }

        let mut raw_ace: *mut c_void = ptr::null_mut();
        if unsafe { GetAce(acl, 0, &mut raw_ace) } == 0 {
            return Err(io::Error::last_os_error())
                .context("failed to inspect the source-session DACL entry");
        }
        if raw_ace.is_null() {
            bail!("source-session DACL entry is null");
        }
        let ace = unsafe { &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>()) };
        if ace.Header.AceType != ACCESS_ALLOWED_ACE_TYPE {
            bail!("source-session DACL entry is not an access-allowed entry");
        }
        if ace.Mask != FILE_ALL_ACCESS {
            bail!("source-session DACL entry does not grant the required private access mask");
        }
        let expected_flags = inheritance_flags(directory) as u8;
        if ace.Header.AceFlags != expected_flags {
            bail!("source-session DACL entry has unexpected inheritance flags");
        }
        let ace_sid = ptr::addr_of!(ace.SidStart).cast_mut().cast::<c_void>();
        if unsafe { IsValidSid(ace_sid) } == 0 {
            bail!("source-session DACL entry has an invalid SID");
        }
        if unsafe { EqualSid(ace_sid, current_user_sid) } == 0 {
            bail!("source-session DACL is not bound to the current user");
        }
        Ok(())
    }

    fn inheritance_flags(directory: bool) -> u32 {
        if directory {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            0
        }
    }

    fn wide_path(path: &Path) -> Result<Vec<u16>> {
        // `canonicalize` returns a verbatim absolute path on Windows. The security APIs otherwise
        // retain the legacy MAX_PATH limit even though Rust's filesystem operations accept the
        // same longer path.
        let canonical = path
            .canonicalize()
            .context("failed to resolve a source-session storage path")?;
        let mut wide = canonical.as_os_str().encode_wide().collect::<Vec<_>>();
        if wide.contains(&0) {
            bail!("source-session path contains a NUL code unit");
        }
        wide.push(0);
        Ok(wide)
    }

    struct CurrentUserSid {
        token_information: AlignedBuffer,
    }

    impl CurrentUserSid {
        fn load() -> Result<Self> {
            let mut token: HANDLE = ptr::null_mut();
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                return Err(io::Error::last_os_error())
                    .context("failed to open the current process token");
            }
            let token = OwnedHandle(token);

            let mut needed = 0_u32;
            unsafe {
                GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
            }
            if needed == 0 {
                return Err(io::Error::last_os_error())
                    .context("failed to size current-user token information");
            }

            let mut token_information = AlignedBuffer::new(needed as usize);
            if unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    token_information.as_mut_ptr(),
                    needed,
                    &mut needed,
                )
            } == 0
            {
                return Err(io::Error::last_os_error())
                    .context("failed to read current-user token information");
            }

            let user = Self { token_information };
            if unsafe { IsValidSid(user.sid()) } == 0 {
                bail!("current process token contains an invalid user SID");
            }
            Ok(user)
        }

        fn sid(&self) -> PSID {
            let token_user = self.token_information.as_ptr().cast::<TOKEN_USER>();
            unsafe { (*token_user).User.Sid }
        }
    }

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    struct AlignedBuffer(Vec<usize>);

    impl AlignedBuffer {
        fn new(byte_length: usize) -> Self {
            let words = byte_length.div_ceil(size_of::<usize>()).max(1);
            Self(vec![0; words])
        }

        fn as_ptr(&self) -> *const c_void {
            debug_assert_eq!((self.0.as_ptr() as usize) % align_of::<usize>(), 0);
            self.0.as_ptr().cast::<c_void>()
        }

        fn as_mut_ptr(&mut self) -> *mut c_void {
            debug_assert_eq!((self.0.as_ptr() as usize) % align_of::<usize>(), 0);
            self.0.as_mut_ptr().cast::<c_void>()
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::bail;

    use super::*;

    #[test]
    fn published_verification_failure_removes_secret_without_exposing_it() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("session.json");
        let secret = "secret-cookie-value-that-must-not-reach-diagnostics";
        fs::write(&path, secret).expect("write rejected fixture");

        let error = verify_published_session_file_with(&path, |_| {
            bail!("simulated privacy verification failure")
        })
        .expect_err("privacy verification failure must reject publication");

        assert!(!path.exists(), "rejected secret file must be removed");
        assert!(!format!("{error:#}").contains(secret));
    }
}
