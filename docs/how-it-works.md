---
title: "How It Works"
description: "The pipeline powering natural language compilation"
order: 1
---

## The Fail-Forward Pipeline

The system uses a pipeline to maximize efficiency and minimize API costs, while retaining human-in-the-loop safety.

### 1. Multi-Model Roles

Four specialized LLM roles, each with a single responsibility, power the engine:

- **Talker**: Translates natural language intent into a structured, precise request for the Doer.
- **Doer**: Automatically generates a robust bash or Python script to perform the task. *Note: Never executes directly!*
- **Checker**: Provides a "dry run" or diff of what changes will happen, shown to the user.
- **Fixer**: If the script fails, the Fixer feeds error logs back to the Doer with full context, creating a self-healing retry loop.

### 2. The Universal Script Cache

Before a single LLM token is spent, the system checks if a perfect script already exists for your machine. 

Using **State-Aware Fingerprinting**, `Ultimate Installer` hashes your exact environment (`Intent + OS + Architecture + Package Manager + Hardware`) and does a fast **Global Bloom Filter** check. 

- **Bloom Hit**: Skips parsing and uses a verified community script.
- **Bloom Miss**: Generate a script using the LLMs and cache it locally for next time. 

Every user-approved script is uploaded to a shared community registry to benefit everyone.
