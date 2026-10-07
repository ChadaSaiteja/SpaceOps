# Windows Storage Intelligence

## Master PRD & Engineering Blueprint

### 1. Project Goal

Build a modern Windows desktop application that helps users understand, analyze, manage, and optimize their disk storage.

The application should:

* Visualize disk usage
* Analyze files and folders
* Find large files
* Search files quickly
* Identify storage-heavy applications
* Identify developer-related storage
* Provide safe cleanup recommendations
* Manage/uninstall applications
* Monitor system resources
* Eventually provide AI-powered storage explanations and recommendations

The product should be:

* Fast
* Native-feeling
* Privacy-first
* Local-first
* Safe
* Modern
* Modular
* Extensible

---

# 2. Product Principle

The application should not simply be another disk-cleaning utility.

The product positioning is:

> **Windows Storage Intelligence**

The application should explain:

> What is using my storage?

and then:

> Why is it using so much?

and finally:

> What can I safely do about it?

---

# 3. Development Philosophy

Do NOT implement the entire application at once.

Follow this sequence:

```text
Idea
 ↓
Market validation
 ↓
PRD
 ↓
Architecture discussion
 ↓
Architecture decision
 ↓
Component planning
 ↓
Component-specific design
 ↓
Phase planning
 ↓
Implementation
 ↓
Testing
 ↓
Review
 ↓
Next phase
```

Every major component must first be discussed and designed.

No implementation should begin until the design for that component is sufficiently clear.

---

# 4. Technology Direction

## Desktop Application

Primary architecture:

```text
Windows Desktop
       │
       ▼
UI Layer
       │
   WinUI 3
       │
       ▼
Application Layer
       │
      C#
       │
       ▼
Native Core
       │
      Rust
       │
       ▼
Windows APIs / Filesystem
```

## Core Technology

Rust is the foundation for performance-sensitive functionality.

Rust should handle:

* Filesystem scanning
* File metadata collection
* Directory traversal
* Storage calculations
* Filesystem indexing
* Search infrastructure
* Duplicate detection
* Cleanup analysis
* Native Windows filesystem interaction
* Potential NTFS/MFT integration
* USN Journal integration
* Core storage engine

## UI

Use:

* WinUI 3
* Windows App SDK
* C#

The UI should communicate with the Rust core through a clearly defined interface.

The exact communication mechanism must be decided during the architecture phase.

Possible approaches:

* Rust DLL + FFI
* C ABI
* IPC
* Tauri-style architecture if evaluated and approved
* Another appropriate Rust/C# bridge

Do not assume the mechanism without discussing performance, maintainability, debugging, packaging, and security.

---

# 5. High-Level Architecture

```text
                    Windows App
                         │
              ┌──────────┴──────────┐
              │                     │
           WinUI 3              Application
              │                    Layer
              │                     │
              └──────────┬──────────┘
                         │
                    Rust Core
                         │
       ┌─────────────────┼──────────────────┐
       │                 │                  │
   Filesystem         Storage            Search
    Engine             Engine             Engine
       │                 │                  │
       ├─────────────────┼──────────────────┤
       │                 │                  │
      NTFS             SQLite            Index
      APIs              DB
       │
       ├── File System
       ├── MFT
       └── USN Journal
```

---

# 6. Major Product Components

The product should be divided into these components.

## A. Application Shell

Responsible for:

* Application startup
* Navigation
* Global state
* Settings
* Error handling
* Notifications
* Theme
* Application lifecycle

---

## B. Filesystem Scanner

Responsible for:

* Selecting drives
* Traversing directories
* Reading file metadata
* Calculating file sizes
* Calculating directory sizes
* Handling permissions
* Handling inaccessible files
* Handling symbolic links/junctions
* Handling system files
* Reporting scan progress
* Cancelling scans
* Parallel scanning

---

## C. Storage Tree

Responsible for representing:

```text
Drive
 └── Directory
      ├── Directory
      │    └── File
      └── File
```

Each node should potentially contain:

```text
id
parent_id
name
path
type
size
file_count
directory_count
extension
modified_time
created_time
attributes
permissions/status
```

The exact model must be discussed before implementation.

---

# 7. Treemap / Storage Visualization

This is one of the core MVP components.

Input:

```text
Filesystem Tree
```

Output:

```text
Interactive Treemap
```

Example:

```text
+---------------------------------------+
|                                       |
|              Users                    |
|                                       |
|                                       |
|--------------------+------------------|
| Program Files      | Docker           |
|                    |                  |
|--------------------+------------------|
| Windows            | Downloads        |
+---------------------------------------+
```

Rectangle size represents storage usage.

Required capabilities:

* Zoom
* Drill down
* Hover information
* File/folder selection
* Breadcrumb navigation
* Parent navigation
* Size display
* Percentage display
* Search-to-location
* Open location
* Context menu

The exact treemap algorithm must be discussed separately.

---

# 8. Storage Analysis

The application should answer:

```text
How much storage is used?
```

Then:

```text
Where is it used?
```

Then:

```text
What type of data is using it?
```

Then:

```text
What can potentially be cleaned?
```

Initial categories:

* Applications
* Documents
* Downloads
* Videos
* Images
* Developer files
* Temporary files
* Caches
* Archives
* System files
* Other

Category detection rules must be designed separately.

---

# 9. File Search

Search should eventually support:

```text
filename
extension
path
size
date
file type
```

Examples:

```text
*.zip
*.iso
*.log
>10GB
modified:30days
node_modules
docker
```

Initial MVP search can be simpler.

The search architecture should eventually support the local filesystem index.

---

# 10. Database / Index

SQLite is the initial database candidate.

Potential entities:

```text
File
Directory
Drive
Scan
Application
CleanupCandidate
```

Example:

```text
Drive
 └── Directory
      └── File
```

The exact schema must be designed separately before implementation.

Important questions:

* How many records can exist?
* How much memory should be used?
* What should remain in memory?
* What should remain in SQLite?
* How should indexes work?
* How should updates happen?
* How should deleted files be handled?
* How should rescanning work?

---

# 11. Incremental Scanning

Initial MVP:

```text
User starts scan
       ↓
Filesystem scan
       ↓
Build storage tree
```

Later:

```text
Initial scan
       ↓
Local index
       ↓
USN Journal
       ↓
Detect changes
       ↓
Update index
```

This should eventually eliminate unnecessary full scans.

USN Journal integration should be considered a later phase, not an MVP requirement.

---

# 12. Cleanup Engine

The cleanup engine should NOT directly delete files based on simple rules.

Architecture:

```text
Scanner
   ↓
Analyzer
   ↓
Cleanup Candidate
   ↓
Risk Classification
   ↓
Explanation
   ↓
User Confirmation
   ↓
Deletion
```

Every cleanup candidate should contain:

```text
What is it?
Where is it?
How large is it?
Why is it considered removable?
What happens if removed?
Risk level
Recommended action
```

Example:

```text
npm Cache

Size: 8.4 GB

Reason:
Cached npm packages.

Effect:
Packages may need to be downloaded again.

Risk:
Low

[Review] [Delete]
```

---

# 13. Developer Storage Analyzer

This is an important product differentiation area.

Potential targets:

```text
node_modules
npm cache
pnpm store
Yarn cache
Docker
WSL
NuGet
Gradle
Maven
pip cache
Python environments
Cargo
Rust target folders
Visual Studio
Android SDK
Git
.NET
VS Code
```

The system should identify these using explicit rules.

Do not use AI initially.

Rules should be deterministic.

---

# 14. Application Manager

Potential capabilities:

* Installed application discovery
* Application size
* Application location
* Related files
* Cache detection
* Application uninstall
* Leftover detection

Windows sources may include:

* Registry uninstall entries
* MSI
* MSIX/AppX
* Program Files
* AppData
* ProgramData
* Application shortcuts

This component requires its own architecture discussion because incorrect cleanup can damage applications.

---

# 15. Duplicate Finder

Later-phase feature.

Potential approach:

```text
File metadata
      ↓
Group by size
      ↓
Group by extension
      ↓
Partial hash
      ↓
Full hash
      ↓
Duplicate groups
```

Do not hash every file immediately.

Hashing strategy should be designed before implementation.

---

# 16. System Monitoring

Potential metrics:

* CPU
* Memory
* Disk
* Network
* GPU
* Battery
* Processes

This should be a separate module from storage analysis.

MVP may only include basic:

```text
CPU
RAM
Disk
```

---

# 17. AI Layer

AI should NOT be part of the initial core architecture.

The local application should work without AI.

Later architecture:

```text
Local Storage Engine
        ↓
Structured Metadata
        ↓
Recommendation Engine
        ↓
Optional AI
```

The AI should receive structured information rather than raw user files.

Example:

```json
{
  "drive": "C:",
  "total": 1000000000000,
  "free": 41000000000,
  "largest_categories": [
    {
      "name": "Docker",
      "size": 83000000000
    }
  ]
}
```

AI can then explain:

> Docker is consuming approximately 83 GB of storage.

---

# 18. Privacy Architecture

Default behavior:

```text
User Computer
      │
      ├── Scan
      ├── Index
      ├── Search
      ├── Analysis
      └── Cleanup
```

These should happen locally.

Server should only handle:

```text
Licensing
Payments
Application updates
Optional crash reporting
Optional AI services
```

Do not upload:

* filenames
* folder structures
* file contents
* personal documents
* scan results

unless the user explicitly enables a feature requiring it.

---

# 19. Website

Website responsibilities:

```text
Landing Page
Features
Screenshots
Pricing
Download
Documentation
Blog
FAQ
Support
Privacy
Terms
```

Suggested stack:

```text
Next.js
TypeScript
Tailwind
Cloudflare
```

Backend requirements are intentionally small.

Potential:

```text
PostgreSQL
License service
Payment provider
Download storage
Sentry
```

---

# 20. Monetization

Initial model:

## Free

* Drive analysis
* Treemap
* Search
* Large-file discovery
* Basic system information

## Pro

* Cleanup
* Developer cleanup
* Duplicate finder
* Application manager
* Advanced search
* Advanced analysis
* Additional features

Possible pricing:

```text
Free

Pro Lifetime
$19.99–$29.99

Optional AI
Subscription
```

Pricing must be validated through market research before finalizing.

---

# 21. MVP Definition

The MVP should NOT contain everything.

MVP objective:

> Prove that users can scan a Windows drive, understand where their storage is being used, and navigate the filesystem through a fast visual interface.

### MVP features

```text
1. Application shell

2. Drive detection

3. Drive selection

4. Filesystem scanner

5. Storage tree

6. Directory/file metadata

7. Storage calculation

8. Treemap visualization

9. Drill-down navigation

10. Breadcrumb navigation

11. File/folder details

12. Largest files

13. Basic search

14. Open file location

15. Basic error handling

16. Scan progress

17. Scan cancellation

18. Basic settings
```

### Not MVP

```text
AI
USN Journal
Advanced cleanup
Application uninstaller
Duplicate finder
Advanced developer cleanup
Cloud sync
Network analysis
Advanced monitoring
Cross-platform support
```

---

# 22. MVP Phase Breakdown

## Phase 0 — Product Validation

Before coding:

* Validate problem
* Identify competitors
* Identify target users
* Identify differentiation
* Validate willingness to pay
* Define MVP

Output:

```text
Market Decision
ICP
Competitor Analysis
MVP Scope
```

---

## Phase 1 — Architecture

Discuss and decide:

* Rust architecture
* C# / WinUI architecture
* Rust/C# communication
* Project structure
* Module boundaries
* Error handling
* Logging
* Configuration
* Threading
* Async model
* Security model

Output:

```text
Architecture Decision Record
System Architecture
Repository Structure
```

---

## Phase 2 — Filesystem Scanner

Design first:

* Directory traversal
* Parallelism
* Metadata collection
* Permissions
* Junctions
* Symbolic links
* Error handling
* Cancellation
* Progress reporting
* Memory usage
* Performance targets

Then implement.

---

## Phase 3 — Storage Tree

Design:

* Node model
* Parent/child relationships
* Size aggregation
* Tree updates
* Memory representation
* Database representation

Then implement.

---

## Phase 4 — Treemap

Design:

* Treemap algorithm
* Layout calculation
* Rendering
* Zoom
* Selection
* Hover
* Navigation
* Large-tree performance

Then implement.

---

## Phase 5 — Search

Design:

* Search index
* Query syntax
* Filename search
* Size search
* Extension search
* Result ranking
* Performance

Then implement.

---

## Phase 6 — Storage Insights

Design:

* File categories
* Large-file detection
* Developer detection
* Cache detection
* Storage summaries

Then implement.

---

## Phase 7 — Cleanup

Design:

* Cleanup rules
* Safety levels
* Permission model
* Confirmation
* Recovery
* Logging

Then implement.

---

## Phase 8 — Application Management

Design:

* Installed application detection
* Application size calculation
* Uninstall mechanisms
* Leftover detection
* Safety model

Then implement.

---

## Phase 9 — Incremental Indexing

Design:

* SQLite indexing
* USN Journal
* Change detection
* Incremental updates
* Database consistency

Then implement.

---

## Phase 10 — System Monitoring

Design:

* CPU
* RAM
* Disk
* Network
* GPU
* Battery
* Process information

Then implement.

---

## Phase 11 — AI

Design:

* Recommendation engine
* Local metadata format
* AI prompt architecture
* Privacy
* Cost control
* Failure handling

Then implement.

---

# 23. Required Engineering Documents

Before implementation, create these documents:

```text
01-product-validation.md

02-product-requirements.md

03-system-architecture.md

04-rust-core-architecture.md

05-windows-integration.md

06-filesystem-scanner-design.md

07-storage-tree-design.md

08-database-design.md

09-treemap-design.md

10-search-design.md

11-cleanup-engine-design.md

12-application-manager-design.md

13-developer-storage-design.md

14-system-monitor-design.md

15-ai-architecture.md

16-security-design.md

17-privacy-design.md

18-testing-strategy.md

19-performance-strategy.md

20-release-strategy.md
```

These documents should evolve during discussions.

---

# 24. Architecture Decision Rule

For every major technical decision:

```text
Problem
↓
Possible solutions
↓
Advantages
↓
Disadvantages
↓
Constraints
↓
Recommendation
↓
Decision
↓
Reason
```

Example:

```text
Problem:
How should Rust communicate with C#?

Option A:
C ABI

Option B:
IPC

Option C:
Other bridge

Compare:

Performance
Complexity
Debugging
Deployment
Safety
Maintainability

Decision:
__________

Reason:
__________
```

Never let an AI silently choose important architecture.

---

# 25. Agent Discussion Process

Each major component gets its own AI discussion.

Example:

```text
Master PRD
     ↓
Filesystem Scanner Agent
     ↓
Scanner Design
     ↓
Review
     ↓
Approve
     ↓
Update PRD
     ↓
Implementation Phase
```

Then:

```text
Treemap Agent
     ↓
Treemap Design
     ↓
Review
     ↓
Approve
     ↓
Update PRD
```

Do the same for:

* Database
* Search
* Cleanup
* Application manager
* Developer analyzer
* Monitoring
* AI

---

# 26. Definition of Done

A phase is not complete just because the code works.

Each phase must contain:

```text
Requirements
Architecture
Design
Implementation
Unit Tests
Integration Tests
Performance Test
Security Review
Error Handling
Documentation
Manual Validation
```

Then:

```text
Phase Review
     ↓
Approved?
   /     \
 No       Yes
 ↓         ↓
Fix     Next Phase
```

---

# 27. Project Repository Structure

Initial proposed structure:

```text
storage-intelligence/
│
├── docs/
│   ├── prd/
│   ├── architecture/
│   ├── decisions/
│   ├── components/
│   └── phases/
│
├── app/
│   └── Windows UI
│
├── core/
│   └── Rust
│
├── tests/
│
├── scripts/
│
├── installer/
│
└── README.md
```

The exact structure should be finalized during the architecture phase.

---

# 28. Engineering Rules

1. Do not implement everything at once.

2. Do not introduce AI where deterministic logic is sufficient.

3. Do not optimize before measuring.

4. Do not delete user files without explicit safety design.

5. Do not make the application dependent on the internet.

6. Keep the Rust core independent from the UI as much as practical.

7. Keep components independently testable.

8. Document major architectural decisions.

9. Every feature must have a clear reason for existing.

10. Every phase must have a clear completion condition.

---

# 29. Product Evolution

```text
MVP
│
├── Scan
├── Tree
├── Treemap
├── Search
└── Large files
       │
       ▼
V1
│
├── Cleanup
├── Developer storage
├── Duplicates
└── Application manager
       │
       ▼
V2
│
├── Incremental indexing
├── USN Journal
├── Advanced monitoring
└── Advanced search
       │
       ▼
V3
│
└── AI Storage Intelligence
       │
       ▼
Future
│
└── Cross-platform
```

---

# 30. Master Success Criteria

The product should eventually allow a user to answer these questions within seconds:

```text
How much space do I have?

What is using my space?

What are my largest folders?

What are my largest files?

What applications consume the most storage?

What developer tools are consuming storage?

What can I safely remove?

How much space can I reclaim?

Why is my disk filling up?

What changed since my previous scan?
```

The application succeeds when it moves from:

> **"Here is a visualization of your disk."**

to:

> **"Here is what is consuming your disk, why it is there, and what you can safely do about it."**
