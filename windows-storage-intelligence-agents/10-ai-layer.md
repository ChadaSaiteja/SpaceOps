# AI Storage Intelligence Design Prompt

Design the optional AI layer.

AI must NOT be required for the core application.

The local application should work fully without AI.

Design how structured local metadata can be transformed into safe AI context.

Potential use cases:

- Explain why a drive is full
- Explain large directories
- Explain developer storage
- Recommend cleanup candidates
- Answer natural-language storage questions
- Explain what a cleanup action does

Do not send raw file contents or personal documents.

Define:

- Data sent to AI
- Data never sent
- Prompt architecture
- Structured context
- Privacy controls
- Cost controls
- Failure handling
- Offline behavior
- Provider abstraction
- Recommendation validation

Do not implement until the privacy and architecture are approved.
