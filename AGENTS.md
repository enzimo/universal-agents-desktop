# Universal Agents Desktop agent instructions

# Agent Driven Project Development

This file is the top-level operating contract for agents working in this repo.
Keep it short, actionable, and stable. Project detail belongs in the project
reference docs such as `README.md`, `QUICKSTART.md`, `TODO.md`, `GOTCHAS.md`,
`plans/` and `docs/` when these directories exist.

## 1. Change Process

### Communicate clearly with the user

- Apply plain language to all user communication: explanations, plans, progress updates, questions, error messages, UI labels, documentation, and completion reports.
- Before you start, say in a line what you're about to do; brief updates while you work help the user follow along. Close with a short recap that stands on its own — what you found, what you did, and what's next — so a reader who only sees the last message has the full picture.
- Lead with the goal or outcome. Say who is doing what, why it matters, and what the user needs to do, if anything. Describe concrete behavior instead of naming an abstract concept and expecting the user to decode it.
- Avoid jargon and engineering metaphors such as "surface", "landed", "envelope", "plumbing", and "wired up". Name the actual thing or action: "available tools", "settings page", "connected the scheduler to the worker", or "delegation boundaries". Choose words that explain the meaning in context; do not merely substitute another obscure term.
- Remove all mannered prose.
- Be precise about progress. Say "implemented", "tested", "committed", "merged", or "deployed" only when that specific step happened. "The fix landed" does not tell the user whether their running instance has changed.
- Use technical terms when they help the user understand or act. Explain unfamiliar terms on first use with a short definition or concrete example. Include exact code or configuration names when needed to locate something, alongside a plain-language explanation.
- Explain blockers as a specific missing condition and a next step. For example: "The lead has no approved rules for granting workers tool access. It can prepare those rules for your review." Do not make the user translate an internal error or design the technical solution themselves.
- Keep sentences direct and focused. Break complicated behavior into understandable steps, preserve important distinctions, and omit implementation details that do not help the user assess the result or make a decision.

### Questions + Research, Impact, Design + Structure Outline, Implement

Follow the general **Questions + Research, Impact, Design + Structure Outline, Implement** pattern when asked to create any new features or code for the user. Individual steps may vary in complexity and may require multiple iterations (loops) depending on the nature of the request.

Before making a change to an existing implementation, ask yourself why the current implementation exists the way it does. After you answer that question, go back and ask it again to see if you have missed anything. Trace the full extent of existing logic and consult any architecture documents so that you deeply understand the rationale and the impact of the changes you are about to make. In situations where the existing logic is complicated or touches many parts of the system, break down your implementation into multiple phases so that the impact of change in each phase is manageable, tracktable and well understood.

For trivial, low-risk, tightly scoped changes such as small doc fixes, obvious typo corrections, or similarly simple edits, agents may compress this process and proceed without waiting for explicit user acknowledgment between each stage. For all other changes, keep the full pattern and user verification steps intact.

After initial request, examine the repo and get back to the user with any additional questions that need to be asked (but don't pester the user if the task is straightforward). Then perform any additional research within the repo, as well as, online, until you have a good understanding of the impacts of the changes and clearly state them to the user. At this point, the user should acknowledge the impact or ask for additional clarification.

Once the user agrees to proceed, present the user with a detailed **design** and a **structured outline** of the implementation and re-evaluate against prior stated impact to make sure the design has not changed the stated impact. Iterate with the user on any points of clarification or redesign.

Before proceeding, clarify the project's testing strategy with the user. Determine whether confidence for this project should lean primarily on unit tests, integration tests, live tests, or a deliberate mix of them.

When the project is still being established, proactively recommend an appropriate testing emphasis based on the system's shape and failure modes. Favor stronger integration or live coverage when important behavior depends on external services, persistence, routing, async workflows, deployment wiring, or other runtime interactions that unit tests may not reveal well.

For each change, define a verification plan that matches that strategy. Generate the most relevant unit tests and any integration / live tests that thoroughly verify the new functionality where practical. Finally, implement the changes exactly as designed. Run the relevant tests at the end, to make sure the implementation is successful and behaves as intended, and document important coverage gaps when full automation is not yet feasible.

## Intent is king
- Make sure you execute to intent rather than to your written plan. If you find during implementation of the plan that it is drifting from the intent, stop and adjust your approach. Executing to intent is **paramount**. We must make sure to verify that the intent is clear and whenever there are deviations in code or plan, to communicate with the user to clarify intent and to change the plan + code to execute to intent.

### Plan before acting
- For any non-trivial task, form a short execution plan and a verification plan before editing.
- Re-plan when new evidence invalidates the current approach.
- Prefer simple, durable solutions over clever patches.

### Fix root causes
- Treat recovered anomalies as defects to investigate, not acceptable outcomes.
- Do not stop at fallback logic if the underlying cause is still unknown.
- If a fallback must remain, keep it explicitly secondary, emit a warning with useful identifiers, and add tests that prove both the primary path and the warning path.
- If testing reveals unanticipated or cascading effects, do not just patch the first visible symptom and continue silently.
- Instead, document what changed, what failed, what secondary effects appeared, and what you believe the causal chain is, then present that report to the user for verification or correction before continuing with broader corrective changes.

### Verify before calling work done
- Do not mark work complete without evidence.
- Run the most relevant tests, checks, or realistic manual validation you can for the change.
- Run linting, compiler, static analysis or any other available tools and examine their output. Fix obvious bugs. When in doubt, ask the user for input on how to resolve issues before fixing. Always surface the actions you took to the user for awareness.
- Compare expected versus actual behavior and resolve mismatches before finishing.

### Keep documentation synchronized
- When behavior, architecture, configuration, or operator workflows change, update the relevant docs in the same change.
- Remove stale instructions, dead compatibility notes, and obsolete knobs instead of letting them accumulate.
- Always document rationale for current architectural and implementation approaches when they are being written. Include enough detail so that future review of the architecture and code can map existing structure to rationale.
- The documents in the `plans\` folder are **living** documents, which means it is imperative that you continuously check them for errors, incomplete or outdated information. Bring any issues up to my attention and ask me whether I want to fix them.

### Write code that is easy to operate
- Prefer clear, maintainable implementations over clever or highly compressed code.
- Actively identify opportunities for abstraction that replaces hard-wired implementation with flexible, modular and common components. Surface opportunities to consolidate code and generate sensible abstractions to the user as you find them, even if they are not directly related to current effort.
- Avoid hardcoding strings when the same string is being reused in code (e.g. in `equals` logic etc). Prefer a common string library that defines the string once and then reuses it across other code blocks.
- Follow existing project patterns unless there is a concrete reason to change them.
- Keep modules and functions focused; split work when responsibilities start to blur.
- Add comments only where they help a future reader understand intent, invariants, or non-obvious behavior.
- Make failure paths observable with structured logging that includes enough context to diagnose production issues.
- Prefer small, explicit changes that preserve surrounding structure unless a broader refactor is required.
- Keep runtime behavior deterministic where practical.
- Avoid silent fallbacks or hidden repair logic; if the runtime has to compensate for anomalous input or state, log it clearly.
- Do not just write a patch to an issue without first understanding whether the patch only masks the underlying problem. Think deeply about **why** the problem exists in the first place and write code that addresses the root cause.
- Remove stale code, outdated compatibility paths, and dead configuration when they no longer serve the current architecture.
- Keep tests aligned with real behavior, especially when changing persisted schemas, message contracts, or recovery flows.
- Review your implementation one more time critically after the original pass and before PR. Pay attention to edge cases or multi-step (e.g. circular dependencies, infinite resource blocks etc) failures. Fix any issues or edge-cases that you uncover.

### Writing Pull (Merge) Requests

- When writing a PR clearly state:
    - The observed problem, bug or requested feature
    - The proposed solution in words
    - The solution as implemented in code (high-level architecture)
    - What edge cases have been considered and tested
    - The tests (unit, integration etc) created to validate the solution and to validate that the solution completely covers the requested work

- Do not include any extraneous information e.g.:
    - "Generated with Claude Code"

### Capture durable lessons in the right place
- Convert repeated mistakes or tricky repo-specific constraints into documentation, tests, or automation.
- Log repo-specific gotchas, unanticipated side effects, and the fixes or mitigations that resolved them in `GOTCHAS.md` so future work can learn from prior incidents.
- Prefer reusable system improvements over one-off reminders in task output.

### Plan carefully to utilize any fitting existing facilities / libraries
- Do not reinvent a feature or process if it can be created with the help of existing functionality inside of the already-used libraries. E.g. For working with agents, always consult the strands sdk documentation first to see if it can be utilized when dealing with tools, turn taking, telemetry etc.

## 2. Project-Specific Guidance

## Shell execution

Route every shell command through RTK. Use a native wrapper such as `rtk git`, `rtk read`, `rtk find`, or `rtk cargo` when available. Use `rtk proxy <command>` for unsupported commands and `rtk proxy sh -c '<commands>'` for compound commands or pipelines.

### Core invariants
- Record hard behavioral, data, API, security, and operational constraints that changes must preserve.
- Use `logging`, not `print`, for runtime output.
- Never put personal details in tracked files or commit metadata: no real names, handles, assistant names, emails, account IDs, domains, machine or runner names, host accounts or paths, Actions run links, or copied chats, emails and logs. Follow `CONTRIBUTING.md` ("Personal details") and commit only with an identity in `.personal-details-allowlist`. Before the first commit in a clone or worktree, run `git config core.hooksPath .githooks`; run `python3 scripts/check_personal_details.py` before every push. If a personal detail was already pushed, stop and tell the user and follow `docs/personal-details-cleanup.md` together with them.

### Architecture and boundaries
- Record major subsystems, external integrations, ownership boundaries, and places where coupling is intentional.

### Design steering
- The design of all UI elements must be consistent within the project. Before generating or adjusting UI elements, read the `ui/DESIGN.md` document and make sure to adhere to the instructions and aesthetics captured there. If the user instructs you to change the design or create new UI elements, confirm whether the DESIGN.md file should be updated to capture user design choices.

### Testing strategy
- Record which test types this project should lean on, what must be covered by integration or live tests, and any required test environments or fixtures.

### Code preferences
- Record repo-specific implementation preferences beyond the general guidance above, such as framework patterns, naming conventions, or observability expectations.

### Change-specific requirements
- Record temporary migration constraints, rollout sequencing, compatibility windows, or other active requirements tied to the current phase of the project.

## Releases
Do not create a release unless the user explicitly asks. Desktop versioning is independent from UA runtime versioning. Production macOS releases require Developer ID signing and notarization; never present an unsigned artifact as a production release.

## 3. Reference Files

Read relevant references first:
- Read `README.md` for the current project overview, architecture, configuration, and testing notes.
- Read `QUICKSTART.md` for the fastest working launch and invocation flow.
- Read `TODO.md` for deferred architectural decisions and known follow-up work.
- Read `GOTCHAS.md` for repo-specific lessons, failure patterns, and mitigations discovered during prior work.
- Read relevant files in `plans/` when active work for the current change has already been outlined there.
- Read relevant files in `docs/` when present for deeper design, operator, or subsystem documentation related to the area being changed.

## 4. What Belongs Where
- Keep this file focused on cross-project behavior and repo-level operating rules.
- Use `docs/` for durable human-facing documentation such as architecture, subsystem behavior, configuration details, and operating procedures.
- Use `plans/` for time-bounded implementation plans, design outlines, migration steps, and other working documents tied to a specific change.
- Use `docs/archive/` and `plans/archive/` only for historical or superseded material that must not be treated as current design.
- Use `data/` for checked-in sample data, fixtures, static assets, schemas, seeds, or other project data files that are part of the repository contract.
- `AGENTS.md` is a **living** document. Update it with project-specific guidance as appropriate but keep it brief and to the point.
