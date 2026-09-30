# AI assistant

Stage 19. Optional, off by default, and bring your own model: OLS ships no model and needs no account.

## Setting up

**Settings → AI assistant**, then turn it on and add a provider. Every provider is the same code over the OpenAI
chat-completions API with a different address and key.

| Provider | Address | Key |
|---|---|---|
| LM Studio (on this computer) | `http://localhost:1234/v1` | none |
| Ollama (on this computer) | `http://localhost:11434/v1` | none |
| Hugging Face | `https://router.huggingface.co/v1` (or your own Inference Endpoint / TGI `/v1` URL) | access token |
| OpenRouter | `https://openrouter.ai/api/v1` | API key; the price of each answer is shown |
| Another server | llama.cpp, vLLM, LocalAI, a company gateway | if it wants one |

**Look on this computer** finds a running LM Studio or Ollama. **Save and test** lists the server's models. Each
feature (explain, config, logs, traffic, commit, palette) can use its own provider; the first provider is the default.
Keys are stored in the Windows credential store, never in a file, and are only sent over https to a server that is not on
this computer.

## What it does

1. **Explain and fix**: an Explain button on diagnostics, the doctor, and a failed setup. The answer says what went wrong and
   why, and may propose steps.
2. **Configs and manifests**: *Draft with AI* on the environment manifest (the draft opens in the editor; nothing is set up
   until you save it and apply the plan), and *Ask AI* on a web config (a change comes back as a diff to apply by hand).
3. **Ask the logs**: the Logs page opens a picker of detected error/warn lines (duplicates collapsed ignoring timestamps, repeat count shown as ×n). Pick one or many, or send the full tail. A pick over ~12k chars spills to a `.log` text file the model pages via `read_excerpt`; the answer quotes the lines it used.
4. **Traffic**: explain a captured request, write a webhook handler, or write a k6 script (saved only if it passes the
   load-test safety scan).
5. **Commit messages** from the staged changes.
6. **Plain-language palette**: "a Laravel site with Redis called shop" becomes a Quick App step to confirm.

Command line: `olsc ai status`, `on`, `off`, `test [provider]`, `ask "..."` (`--feature palette`, `--log web:error`),
`explain <finding>`. Add `--preview` to see what would be sent, `--yes` to allow a provider outside this computer, and
`--apply` to run the proposed steps without asking.

## Privacy and safety

- **Nothing is sent unless you ask.** Every request starts from a button (or `olsc ai`), and **Show what will be sent**
  displays the exact prompt first.
- **Everything is redacted**: `.env` values, passwords, tokens, keys, cookies, signatures and the home folder name, using the
  same rules as the logs and the support bundle. The same applies to what the model reads through its tools.
- **Local first**: a provider outside this computer is labelled everywhere it is used and needs a confirmation for each
  request. With only local providers configured, nothing leaves the machine.
- **The assistant proposes, the core disposes.** A proposal is a list of core commands, checked against an allowlist
  (start/stop/restart a service or site, install a runtime, apply or validate the web config, regenerate a certificate,
  create or back up a database, apply a setup, run Composer install, set one `.env` value, start a Quick App, and a few
  more). Anything else, including shell commands, secrets and deletions, is refused and shown as refused. Nothing runs until
  you tick the steps and press Run; steps that replace something (a `.env` value, an overwrite) need a separate
  confirmation. The allowlist is checked again when the steps run. An imported Quick App never gets its approval from a plan.
- **The model's tools only read**: projects, sites, services, findings, log tails, saved excerpt files (`read_excerpt`), web configs, the manifest, and the names
  in a `.env` file (values hidden). There is no shell tool. A model that rejects tools is retried without them, and a
  provider can be set to never get them.
- The AI commands are never available over the local HTTP API.

## Limits

Models can be wrong; read a proposal before approving it. Small local models may be too weak for config help, which is why
each feature can use a different provider. Answers are shown as plain text, not rendered Markdown. The cost of an answer is
shown for OpenRouter only. The feature has been tested against a mock OpenAI-compatible server, not against a real model.
