# Security Policy

The SpaceOps team takes the security of our application and users' data very seriously. Because SpaceOps interacts deeply with the Windows filesystem, reads metadata, and manages storage cleanup, we maintain strict safety and security guarantees.

---

## Supported Versions

Only the latest release version on the `main` branch receives active security updates:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1.0 | :x:                |

---

## Security-Sensitive Areas

The following areas in SpaceOps are considered security-critical:

1. **Filesystem Deletion Safety**:
   - The system blacklist (`C:\Windows`, `C:\Program Files`, boot directories, user profiles) must be immutable and cannot be bypassed.
   - Deletions must route through the Windows Recycle Bin via COM (`IFileOperation` / `SHFileOperationW`) to prevent unrecoverable data loss.
2. **Reparse Point & Symlink Handling**:
   - Directory junctions and symbolic links must never be traversed recursively to prevent directory traversal loops or unintended multi-volume deletion.
3. **Privilege Boundaries**:
   - Tier 1 NTFS Direct MFT and USN Journal reading require Administrator elevation. The scanner must degrade safely and cleanly when running in standard unprivileged user mode.
4. **C-ABI Memory Safety**:
   - Native Rust code must never trigger undefined behavior, unaligned access, or data races across the FFI boundary into .NET.
   - Unmanaged pointers must be strictly bound to lifetime-managed `SafeHandle` instances.

---

## Reporting a Vulnerability

**Please do NOT disclose security vulnerabilities publicly via GitHub Issues.**

If you discover a potential vulnerability involving:
- Accidental or arbitrary file deletion
- Path traversal or junction escape
- Privilege escalation
- Memory corruption across the C-ABI boundary
- Malicious command execution during application uninstallation

Please report it privately:

1. **GitHub Private Security Advisory**: Use the **Security** tab on the repository and click **"Report a vulnerability"** (preferred).
2. **Direct Security Contact**: Email **saiteja.chada2000@gmail.com** with the subject line `[SECURITY] SpaceOps Vulnerability Report`.

### What to Include in Your Report:
- Detailed description of the vulnerability and security impact.
- Clear reproduction steps or proof-of-concept (POC) script.
- Specific Windows version and build number.
- Any suggested fixes or mitigations.

### Response Timeline:
- **Initial Acknowledgement**: Within 48 hours.
- **Triage & Assessment**: Within 5 business days.
- **Fix & Public Advisory**: Coordinated disclosure after a patch has been verified and released.
