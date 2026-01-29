# Sparkle

Sparkle is an AI collaboration framework that transforms your agent from a helpful assistant into a thinking partner. It learns your working patterns over time and maintains context across sessions.

## Quick Reference

| What | How |
|------|-----|
| Activate | Automatic when mod is enabled |
| Teach a pattern | Say "meta moment" during a session |
| Save session | Use `/checkpoint` before ending |
| Local state | `.sparkle-space/` (add to .gitignore) |
| Persistent learnings | `~/.sparkle/` |

## How It Works

**Automatic activation** - When the Sparkle mod is enabled, it activates automatically when you create a new thread. No manual setup required.

**Local workspace state** - Sparkle creates a `.sparkle-space/` directory in your workspace to store working memory and session checkpoints. Add this to your `.gitignore`.

**Persistent learnings** - Pattern anchors and collaboration insights are stored in `~/.sparkle/` and carry across all your workspaces.

**Pattern anchors** - These are exact phrases that recreate collaborative patterns. Sparkle learns these over time as you work together, capturing what works well in your collaboration style.

**Teaching patterns** - During a session, say "meta moment" to pause and examine what's working. Sparkle will capture the insight as a pattern anchor or collaboration evolution that future sessions can build on.

**Closing out** - Use `/checkpoint` to save session learnings before ending. This preserves your progress and creates continuity for the next session.

## Learn More

For full documentation on Sparkle's collaboration patterns and identity framework, see the [Sparkle documentation](https://sparkle-ai-space.github.io/sparkle-mcp/).
