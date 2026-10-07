# Storage Tree Design Prompt

Design the filesystem storage-tree model.

Do NOT write implementation code yet.

The tree must represent:

Drive
└── Directory
    ├── Directory
    └── File

Analyze:

- Node model
- Parent/child relationships
- IDs
- Paths
- File metadata
- Directory metadata
- Aggregated sizes
- File counts
- Directory counts
- Extension/type information
- Incremental updates
- Memory representation
- SQLite representation
- Large-tree behavior
- Millions of nodes
- Sorting
- Lazy loading
- Thread safety

Define the Rust data model and the boundary exposed to the UI.

Compare alternatives and recommend one.

Do not implement until the design is approved.
