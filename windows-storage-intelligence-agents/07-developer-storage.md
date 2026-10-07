# Developer Storage Analysis Prompt

Design the developer-storage analysis system.

Target users include software developers using Windows.

Analyze:

- Node.js
- npm
- pnpm
- Yarn
- Docker
- WSL
- Visual Studio
- .NET
- NuGet
- Python
- pip
- Java
- Maven
- Gradle
- Android
- Rust
- Cargo
- Git
- VS Code

For each ecosystem determine:

- What consumes storage?
- Where is it located?
- How can it be detected?
- Can it be safely cleaned?
- What are the risks?
- What happens after cleanup?
- What metadata should be displayed?

Use deterministic rules first.

Do not introduce AI unless there is a clear reason.

Create the architecture and detection-rule model before implementation.
