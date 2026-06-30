---
name: agent-pin
description: Use this skill when the user wants to pin important agent output to the desktop, or when a task result is better shown as a lightweight desktop pin instead of a long chat response. Supports markdown, image, and status pins through the local agent-pin CLI.
---

# Agent Pin Skill

## Purpose

Use this skill to send important Agent output to the user's desktop as a floating Pin window.

A Pin is a lightweight desktop window, similar to a PixPin-style pinned image or note. It is for display only in the MVP. Do not expect user interaction or feedback.

## When to Use

Use this skill when:

- The user explicitly asks to pin something to the desktop.
- A long task completes and the result should remain visible.
- A high-risk issue or important conclusion should not be buried in chat.
- An image result should be shown on the desktop.
- A concise summary should stay visible while the user works.
- The user asks for a lightweight visual reminder.

Do not use this skill for:

- Ordinary chat replies.
- Internal reasoning.
- Repeated low-value updates.
- Unverified guesses.
- Sensitive private information unless the user requests it.
- Full chat transcripts.

## Supported Pin Content

MVP supports:

- markdown
- image
- status

Tables should be written as Markdown tables inside a markdown block.

MVP does not support:

- choice
- user feedback events
- interactive buttons
- full HTML artifacts
- remote sharing

## Preferred Usage

Prefer the `agent-pin` CLI.

Check whether Agent Pin is running:

```bash
agent-pin health
```

### Markdown Pin

Use a file when content is more than one short sentence:

```bash
agent-pin markdown --title "PR Review Result" --file ./review.md
```

Use text only for short content:

```bash
agent-pin markdown --title "Conclusion" --text "The first version should focus on desktop pins only."
```

### Image Pin

```bash
agent-pin image --title "Design Reference" --path ./image.png --caption "Reference image"
```

### Status Pin

```bash
agent-pin status --title "Task Complete" --level success --text "Review completed. Found 2 issues."
```

Allowed status levels:

```text
info
success
warning
error
```

### Mixed Pin

Create a JSON file and push it:

```bash
agent-pin push --file ./pin.json
```

Example `pin.json`:

```json
{
  "version": 1,
  "title": "Review Summary",
  "blocks": [
    {
      "type": "markdown",
      "content": "## Conclusion\nFound 2 issues. Fix the first one first."
    },
    {
      "type": "image",
      "path": "C:/Users/hao/Desktop/error.png",
      "caption": "Error screenshot"
    }
  ]
}
```

## Fallback

If CLI is unavailable, call the local HTTP API:

```http
POST http://127.0.0.1:4317/api/pins
```

If HTTP is unavailable, write a pin JSON file to:

```text
~/.agent-pin/inbox/
```

## Content Rules

- Keep pins concise.
- Do not pin the entire conversation.
- Prefer summaries, decisions, risks, statuses, and final results.
- One pin should have one clear purpose.
- Do not create choice pins in MVP.
- Do not wait for user response after pinning.
- If the user asks to pin a long result, summarize it first unless they explicitly request the full text.

## Good Pin Examples

Good:

```text
Title: PR 审查结果
Content: 发现 2 个问题，建议先修 shared exports 指向源码的问题。
```

Good:

```text
Title: TryCue 审查完成
Status: success
Text: 已完成后端迁移检查，未发现阻塞问题。
```

Bad:

```text
Title: 全部聊天记录
Content: <entire conversation transcript>
```

## Important

The MVP is display-only. Do not assume button clicks, events, or feedback are available.
