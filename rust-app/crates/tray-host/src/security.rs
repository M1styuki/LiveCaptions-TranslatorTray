use crate::native::{handle, os_error, own};
use std::{io, os::windows::io::OwnedHandle};
use windows::Win32::{
    Foundation::{GENERIC_ALL, HANDLE},
    Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, GetLengthSid,
        GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor, PSECURITY_DESCRIPTOR,
        SE_DACL_PROTECTED, SECURITY_DESCRIPTOR, SetSecurityDescriptorControl,
        SetSecurityDescriptorDacl, TOKEN_GROUPS, TOKEN_QUERY, TokenLogonSid,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

// The absolute descriptor references the heap-backed ACL. Both allocations stay
// at stable addresses when this value moves, and CreateNamedPipe copies them
// before PipeSecurity is dropped. The ACL contains its own copy of the SID.
pub struct PipeSecurity {
    descriptor: Box<SECURITY_DESCRIPTOR>,
    _acl: Vec<usize>,
}

impl PipeSecurity {
    pub fn current_logon() -> io::Result<Self> {
        // SAFETY: process pseudo-handle is borrowed; token is returned with one owned reference.
        let token: OwnedHandle = unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(os_error)?;
            own(token)
        };
        let mut size = 0;
        // SAFETY: size query intentionally provides an empty output buffer.
        unsafe {
            let _ = GetTokenInformation(handle(&token), TokenLogonSid, None, 0, &mut size);
        }
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        // usize storage provides alignment for TOKEN_GROUPS and its embedded pointers.
        let mut buffer = vec![0_usize; (size as usize).div_ceil(size_of::<usize>())];
        // SAFETY: buffer has at least size bytes, correctly aligned, and remains
        // live through AddAccessAllowedAce, which copies the SID into our ACL.
        unsafe {
            GetTokenInformation(
                handle(&token),
                TokenLogonSid,
                Some(buffer.as_mut_ptr().cast()),
                size,
                &mut size,
            )
            .map_err(os_error)?;
            let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
            if groups.GroupCount != 1 {
                return Err(io::Error::other("Cannot determine current logon SID"));
            }
            let sid = groups.Groups[0].Sid;
            let bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>()
                + GetLengthSid(sid) as usize;
            let mut acl = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
            let acl_pointer = acl.as_mut_ptr().cast::<ACL>();
            InitializeAcl(acl_pointer, bytes as u32, ACL_REVISION).map_err(os_error)?;
            AddAccessAllowedAce(acl_pointer, ACL_REVISION, GENERIC_ALL.0, sid).map_err(os_error)?;
            let mut descriptor = Box::<SECURITY_DESCRIPTOR>::default();
            let pointer = PSECURITY_DESCRIPTOR(std::ptr::from_mut(descriptor.as_mut()).cast());
            // SECURITY_DESCRIPTOR_REVISION is 1 in the Windows SDK. No textual
            // SDDL parser/provider initialization is needed for this one ACE.
            InitializeSecurityDescriptor(pointer, 1).map_err(os_error)?;
            SetSecurityDescriptorDacl(pointer, true, Some(acl_pointer), false).map_err(os_error)?;
            SetSecurityDescriptorControl(pointer, SE_DACL_PROTECTED, SE_DACL_PROTECTED)
                .map_err(os_error)?;
            // Same D:P(A;;GA;;;<current logon SID>) policy as before: one
            // non-inheriting allow ACE, protected DACL, no broad principals.
            Ok(Self {
                descriptor,
                _acl: acl,
            })
        }
    }

    pub fn descriptor(&self) -> PSECURITY_DESCRIPTOR {
        PSECURITY_DESCRIPTOR(
            std::ptr::from_ref(self.descriptor.as_ref())
                .cast_mut()
                .cast(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::{
                ACL_SIZE_INFORMATION, AclSizeInformation,
                Authorization::{
                    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    SDDL_REVISION_1,
                },
                EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorControl,
                GetSecurityDescriptorDacl, GetSecurityDescriptorGroup, GetSecurityDescriptorOwner,
                IsValidSecurityDescriptor, PSID, SE_DACL_PRESENT, SE_SELF_RELATIVE,
            },
        },
        core::{BOOL, PCWSTR, PWSTR},
    };

    struct LocalDescriptor(PSECURITY_DESCRIPTOR);
    impl Drop for LocalDescriptor {
        fn drop(&mut self) {
            // SAFETY: the test owns the allocation returned by the SDDL parser.
            let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }

    fn policy(descriptor: PSECURITY_DESCRIPTOR, expected_sid: PSID) -> (u16, Vec<u8>) {
        // SAFETY: both supplied buffers are live throughout this inspection.
        unsafe {
            assert!(IsValidSecurityDescriptor(descriptor).as_bool());
            let mut control = 0;
            let mut revision = 0;
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision).unwrap();
            assert_eq!(revision, 1);
            assert_eq!(control & SE_DACL_PRESENT.0, SE_DACL_PRESENT.0);
            assert_eq!(control & SE_DACL_PROTECTED.0, SE_DACL_PROTECTED.0);
            let mut present = BOOL(0);
            let mut defaulted = BOOL(1);
            let mut acl = std::ptr::null_mut();
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted).unwrap();
            assert!(present.as_bool());
            assert!(!defaulted.as_bool());
            assert!(!acl.is_null()); // NULL would allow Everyone.
            let mut info = ACL_SIZE_INFORMATION::default();
            GetAclInformation(
                acl,
                std::ptr::from_mut(&mut info).cast(),
                size_of_val(&info) as u32,
                AclSizeInformation,
            )
            .unwrap();
            assert_eq!(info.AceCount, 1);
            let mut raw = std::ptr::null_mut();
            GetAce(acl, 0, &mut raw).unwrap();
            let ace = &*raw.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(ace.Header.AceType, 0); // ACCESS_ALLOWED_ACE_TYPE
            assert_eq!(ace.Header.AceFlags, 0); // no inheritance
            assert_eq!(ace.Mask, GENERIC_ALL.0);
            EqualSid(
                PSID(std::ptr::from_ref(&ace.SidStart).cast_mut().cast()),
                expected_sid,
            )
            .unwrap();
            let bytes =
                std::slice::from_raw_parts(raw.cast::<u8>(), ace.Header.AceSize as usize).to_vec();
            let mut owner = PSID::default();
            let mut group = PSID::default();
            GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted).unwrap();
            GetSecurityDescriptorGroup(descriptor, &mut group, &mut defaulted).unwrap();
            assert!(owner.0.is_null());
            assert!(group.0.is_null());
            (control & !SE_SELF_RELATIVE.0, bytes)
        }
    }

    #[test]
    fn binary_pipe_policy_matches_original_sddl_and_survives_move() {
        // Obtain an independent expected logon SID while its token buffer lives.
        unsafe {
            let mut raw = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw).unwrap();
            let token = own(raw);
            let mut size = 0;
            let _ = GetTokenInformation(handle(&token), TokenLogonSid, None, 0, &mut size);
            assert!(size > 0);
            let mut groups = vec![0_usize; (size as usize).div_ceil(size_of::<usize>())];
            GetTokenInformation(
                handle(&token),
                TokenLogonSid,
                Some(groups.as_mut_ptr().cast()),
                size,
                &mut size,
            )
            .unwrap();
            let groups = &*groups.as_ptr().cast::<TOKEN_GROUPS>();
            assert_eq!(groups.GroupCount, 1);
            let sid = groups.Groups[0].Sid;
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(sid, &mut text).unwrap();
            let sid_string = text.to_string();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            let sddl = crate::native::wide(format!("D:P(A;;GA;;;{})", sid_string.unwrap()));
            let mut original = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut original,
                None,
            )
            .unwrap();
            let original = LocalDescriptor(original);
            let binary = PipeSecurity::current_logon().unwrap();
            let pointer_before_move = binary.descriptor().0;
            let moved = Box::new(binary);
            assert_eq!(moved.descriptor().0, pointer_before_move);
            assert_eq!(policy(moved.descriptor(), sid), policy(original.0, sid));
        }
    }
}
