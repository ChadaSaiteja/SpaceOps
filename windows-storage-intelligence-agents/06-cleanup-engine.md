# Cleanup Engine Design Prompt

Design the cleanup engine.

Do NOT write deletion code yet.

The cleanup system must prioritize safety.

Design:

- Cleanup candidate detection
- Rules
- Risk levels
- Protected locations
- User confirmation
- Permission handling
- File locking
- Recovery strategy
- Logging
- Undo possibilities
- Dry-run mode
- Developer cleanup
- Cache cleanup
- Temporary files
- Application leftovers

Every cleanup candidate must explain:

- What it is
- Where it is
- How much space it uses
- Why it can potentially be removed
- What happens after removal
- Risk level

Define the safety model before implementation.

Do not allow deletion logic to be implemented until the design is approved.
