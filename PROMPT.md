# Add an application to SproutOS

Copy the prompt below into Codex, Claude Code, or another coding agent. The agent should run from
an existing checkout of this repository when possible; otherwise it can create a fresh clone.

```text
Help me contribute a new application to the SproutOS Deployment Templates catalogue:

https://github.com/MySproutOS/Deployment-Templates

This is an implementation task. Continue through environment setup, implementation, meaningful
tests, a disposable real deployment, and browser verification. Do not stop after writing a plan.

Start by asking me only:

“Do you already have a SproutOS account and organization?”

Wait for my answer before continuing.

If I do not have an account:

1. Open https://sproutos.me/login using the available browser integration.
2. Ask me to complete the sign-up or sign-in flow myself.
3. Resume only after I confirm that I am signed in.

Never ask me to paste OAuth codes, API keys, session cookies, passwords, or other credentials into
chat.

Next, ask me:

“Would you like me to find and recommend an open-source website or application, or do you have a
GitHub repository URL you want to use?”

Wait for my choice before continuing.

If I provide a GitHub URL, inspect that repository and assess it before editing anything. If I ask
you to find an application, research current candidates and present three suitable choices. For
each choice, include its GitHub repository, license, recent maintenance activity, what it does,
deployment requirements, whether SproutOS can support its complete feature set, and any material
blockers. Prefer useful, actively maintained applications with a clear open-source license whose
full functionality can run on SproutOS. Let me choose the application; do not silently choose one
for me.

Initialize the working environment:

1. If this repository is already checked out, inspect its status, remotes, current branch, and
   worktrees before changing it. Preserve unrelated work. Otherwise clone it from the URL above.
2. Fetch `origin` and base a normal feature branch on the latest `origin/main`. Do not base the
   contribution on an old local feature branch.
3. Read the current README, SECURITY policy, schemas, protocol, CI and publication workflows, and
   the existing application recipes. Treat the current repository as authoritative rather than
   relying on remembered commands or layouts.
4. Inspect the pinned Rust toolchain and install only missing prerequisites. Ask before privileged
   or system-wide installation.
5. Read the current SproutOS coding-agent instructions at
   https://sproutos.me/skills/sproutos/SKILL.md. Do not commit the downloaded skill to this
   repository.
6. Check for the SproutOS CLI with `sprout --version`. If it is missing or obsolete, follow the
   current instructions at https://sproutos.me/docs/cli and the official SproutOS GitHub release.
   Download the archive for this operating system and verify it against both `SHA256SUMS` and the
   release manifest before installing it. Do not hard-code an old CLI version.
7. Run `sprout auth login`, `sprout auth status`, and `sprout org list`. Browser authentication is a
   human checkpoint: open the authorization page using Claude in Chrome, the Codex Chrome/browser
   integration, or an equivalent browser tool, and let me complete authentication. If I can access
   more than one organization, ask which one to use, then run `sprout org use <slug>`.
8. Run `sprout region list` and use a currently available region for later acceptance work.

Before editing, assess the chosen application:

- Pin an exact 40-character upstream commit.
- Confirm that its license permits the intended redistribution and modification.
- Build a feature inventory from the pinned source and its current documentation. Include every
  user-facing and operational function, including authentication, administration, uploads and
  downloads, background work, schedules, email, realtime behavior, imports and exports, external
  integrations, search, migrations, backups, and multi-user or multi-instance behavior where the
  application provides them. Do not redefine “all functionality” to mean only the easiest happy
  path or the features exercised by an existing smoke test.
- Determine its build output, runtime, architecture, health route, migrations, persistent storage,
  environment variables, generated secrets, and required SproutOS services.
- Create a compatibility matrix mapping every inventoried function to its runtime, storage,
  network, protocol, service, secret, and lifecycle requirements and to the exact SproutOS feature
  that satisfies each requirement. Distinguish source inspection, local proof, deployed proof, and
  anything still unverified.
- Verify each relevant SproutOS capability from current public documentation, current platform
  implementation, and existing acceptance evidence before writing recipe code or creating any
  deployment resources. The presence of a similarly named platform capability is not proof that it
  satisfies the application's requirement. Record the exact supporting source or evidence for each
  compatibility conclusion. The later real deployment must confirm that the completed integration
  actually exercises those capabilities.
- Identify requirements such as durable local disk, unrestricted background processes, WebSockets
  or SSE, unsafe startup migrations, unsupported native architectures, or unavailable managed
  services.
- If any function cannot be supported or verified, explain the exact blocker and ask whether to
  pursue a separate SproutOS change or choose a different application. Do not silently remove,
  disable, degrade, or omit functionality to make the recipe appear compatible.
- Turn the compatibility matrix into an application-specific acceptance checklist that exercises
  every function through the interface a real user or operator uses. A single representative user
  journey is insufficient, and a health response is not functional proof.

This assessment is a hard gate. Present the completed feature inventory, compatibility matrix,
supporting SproutOS evidence, blockers, and proposed acceptance checklist to me before implementing
the recipe or attempting a deployment. Continue only after every application requirement is either
supported or I explicitly choose how to resolve it. Do not start implementation merely because the
application builds locally, and do not use a trial deployment to discover platform requirements
that source and platform inspection should have identified first.

Implement the recipe using the repository's current patterns:

- Add `apps/<app-id>/manifest-source.json`.
- Add one `sprout-template-<app-id>` Rust plugin under `plugins/<app-id>`.
- Add only the deterministic transformations, build and runtime assets, and generated GitHub
  deployment workflow required by the pinned source.
- Update the upstream lock and any tests or production-evidence configuration required by the
  current repository.
- Let Cargo and CI discover the plugin through the existing conventions.
- Do not hand-edit generated catalogue output, provenance, OCI digests, or release-produced plugin
  locks.
- Never include secret values, SproutOS credentials, GitHub credentials, or cloud credentials in
  the manifest, plugin request, generated repository, workflow, fixtures, or evidence.
- Pin external GitHub Actions to full commit SHAs.
- Make a second plugin application produce no changes.
- Refuse a different repository, upstream commit, malformed request, or unexpected source bytes.
- Keep the manifest `blocked` unless exact live acceptance evidence satisfying the current
  repository policy exists. Never manufacture or copy evidence from another application.

Run the complete focused validation required by the current repository, including:

- `cargo fmt --all --check`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --all-targets --locked`;
- shell syntax, ShellCheck, and actionlint for affected scripts and workflows;
- the new recipe's pinned-upstream integration test;
- a first plugin application with the exact expected diff;
- a second application proving idempotence;
- the real upstream build for the target Linux ARM64 runtime; and
- application-specific migration, startup, security, and persistence tests.

Do not treat local tests as deployment evidence.

After the local checks pass, explain the disposable GitHub and SproutOS resources that the live
test will create and confirm with me before creating anything billable. Then perform a real
deployment if my account and repository permissions allow it:

1. Create a disposable GitHub repository from the exact pinned upstream commit and apply the new
   plugin.
2. Connect that repository to a disposable SproutOS project in a region returned by
   `sprout region list`.
3. Provision only the services declared by the recipe.
4. Push the generated source and let its generated workflow build and deploy through GitHub OIDC.
5. Wait for both the GitHub Actions run and SproutOS deployment to reach terminal success. A queued
   job, successful build, or project-creation job is not a successful deployment.
6. Record the source commit, plugin digest, repository, workflow run URL, project and deployment
   identifiers, generated hostname, and deployment state.
7. Open the exact generated hostname with the available Chrome/browser integration.
8. Execute the complete acceptance checklist derived from the feature inventory. Use the browser
   for visible user and administrator behavior, and the appropriate real interface for APIs,
   background jobs, schedules, email, storage, imports, exports, search, realtime behavior, and
   external integrations. Do not substitute mocks for a deployed dependency.
9. For every stateful feature, create uniquely identifiable data, refresh or reconnect, and confirm
   that it remains and is isolated correctly. Exercise multi-user and permission boundaries when
   the application supports them.
10. Trigger a second deployment from the same tested source and repeat the checklist portions
    affected by startup, migrations, stored data, generated secrets, or deployment replacement.
    Confirm that previously created data and identities still work.
11. Inspect SproutOS logs for the exercised requests and background work and confirm that the
    expected project and services handled them without hidden runtime failures.
12. Verify every declared managed service through the application's real behavior as well as a
    harmless direct read and write where appropriate.
13. Update the compatibility matrix with the exact evidence for each function. Anything not
    exercised remains unverified; do not describe the application as fully supported or eligible
    for a live listing while any function is unsupported or unverified.
14. Capture sanitized evidence without passwords, tokens, cookies, connection strings, or secret
    environment values.

A `curl` response or health endpoint may supplement browser evidence but cannot replace it. If no
browser integration is available, tell me exactly which browser capability is missing and ask me
to enable it. Do not claim browser acceptance without browser evidence.

Keep publication boundaries explicit:

- Prepare the contribution on a feature branch and offer to open a draft pull request.
- Do not merge the pull request.
- Do not publish OCI artifacts, import the public catalogue, or change a listing to `live` without
  maintainer authorization and qualifying evidence.
- If contributor permissions cannot run the protected publication path, leave the listing honestly
  blocked and provide maintainers with the exact remaining acceptance steps.
- Ask whether I want disposable GitHub and SproutOS resources retained for review or deleted. Do
  not delete them without my answer.
- Do not modify the SproutOS repository or vendor Deployment-Templates as a submodule unless a
  cross-repository contract change is proven necessary and I explicitly authorize that scope.

At completion, report:

- the application and pinned upstream commit;
- the files and behavior added;
- every validation command and result;
- GitHub workflow and SproutOS deployment identifiers;
- what the browser test proved;
- whether the listing is blocked or eligible for live publication and why;
- any remaining maintainer-only action; and
- disposable resources that remain and their cleanup status.
```
