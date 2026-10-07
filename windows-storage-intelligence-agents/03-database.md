# Storage Database Design Prompt

Design the local database/index for the Windows Storage Intelligence application.

Do NOT write code yet.

Evaluate SQLite and alternative approaches.

Design how we store:

- Drives
- Directories
- Files
- Metadata
- Scan information
- Application information
- Cleanup candidates
- Index state

Analyze:

- Schema
- Primary keys
- Foreign keys
- Indexes
- Query patterns
- Database size
- Millions of files
- Incremental updates
- Deleted files
- Rescans
- Transactions
- Concurrency
- Crash recovery
- Migration strategy

The database must support future USN Journal incremental indexing.

Produce the database design first.

Only after approval should we create the implementation plan.
