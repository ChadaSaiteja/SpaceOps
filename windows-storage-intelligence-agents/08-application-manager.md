# Application Manager Design Prompt

Design the Windows application-management component.

Do NOT implement it yet.

Analyze:

- Registry uninstall entries
- MSI
- MSIX/AppX
- Microsoft Store applications
- Program Files
- Program Files (x86)
- AppData Local
- AppData Roaming
- ProgramData
- Shortcuts
- Application caches
- Leftovers

Design:

- Installed application discovery
- Application identity
- Application size
- Installation location
- Uninstall mechanisms
- Related-file detection
- Leftover detection
- Permission handling
- Safety model
- User confirmation
- Logging

Do not assume that deleting a similarly named folder is a valid uninstall method.

Produce the architecture, data model, safety model, and implementation phases before code.
