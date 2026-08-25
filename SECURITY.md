# Security Policy

## Reporting a Vulnerability

We take the security of Shovel seriously. If you discover a security vulnerability, please report it privately through **GitHub Security Advisories**:

1. Go to the [Security Advisories](https://github.com/Fynth/Shovel/security/advisories) page of this repository.
2. Click **"New draft security advisory"**.
3. Fill out the form with details about the vulnerability.

This ensures the issue is handled privately until a fix can be released. Please do **not** report security vulnerabilities through public GitHub issues, discussions, or pull requests.

## Supported Versions

Only the **latest stable release** is supported with security updates. We do not provide backports or patches for older versions.

| Version | Supported |
|---|---|
| Latest release | ✅ |
| Older releases | ❌ |

## What to Include

When reporting a vulnerability, please include as much of the following as possible:

- A clear description of the vulnerability
- Steps to reproduce the issue
- Potential impact
- Any suggested fixes or mitigations (if known)

## Response Timeline

We aim to respond to security reports within **48 hours**. Our response will include:

1. Confirmation that we have received the report.
2. An initial assessment of the severity and impact.
3. An estimated timeline for a fix.

## No PGP Key

We do not require (or provide) a PGP key for security reports. All reports should be submitted through GitHub's Security Advisory system, which provides encrypted communication.

## Disclosure Policy

We follow a coordinated disclosure process:

1. The reporter submits a vulnerability through GitHub Security Advisories.
2. We work with the reporter to understand and validate the issue.
3. A fix is prepared and tested.
4. Once a fix is released (typically as part of a new version), the vulnerability may be disclosed publicly.

We ask that you allow us reasonable time to address the issue before any public disclosure.

## Scope

This security policy covers the Shovel application and its official packages (Flatpak, APT, Arch Linux, AUR, Windows). Issues within third-party dependencies should be reported to the respective project.
