# Filesystem Scanner Design Prompt

We are designing the Filesystem Scanner for a Windows Storage Intelligence application.

Do NOT write implementation code yet.

Design the scanner completely first.

Analyze:

- Windows filesystem APIs
- NTFS considerations
- Directory traversal
- File metadata
- Directory metadata
- Permissions
- Access-denied files
- Junctions
- Symbolic links
- Reparse points
- System files
- Sparse files
- Hard links
- File size semantics
- Parallel scanning
- Threading
- Async behavior
- Cancellation
- Progress reporting
- Error handling
- Memory usage
- Large drives
- Millions of files
- Performance bottlenecks
- Rust architecture
- Communication with the UI
- Test strategy

Compare implementation approaches.

Define:

1. Scanner architecture
2. Rust modules
3. Data structures
4. APIs/interfaces
5. Threading model
6. Error model
7. Progress model
8. Cancellation model
9. Performance targets
10. Testing strategy

Do not make major assumptions without explaining them.

After the design is approved, create a detailed implementation phase plan.

Only after I approve that plan should implementation begin.
