---
name: advisaur-onboard
description: One-time onboarding interview that seeds Advisaur MCP with foundational memories about the user. Run this when a user first installs Advisaur MCP, says "get to know me", "onboard", "set up my memory", or asks why their memory is empty. Also triggers when thought_stats shows zero or very few thoughts.
---

# Advisaur Onboard — First-Run Memory Seeding

You're running the Advisaur MCP onboarding interview. Your goal: learn enough about the user to make their AI tools immediately useful — then save it all as structured memories.

This is a conversation, not a form. Ask a few questions, listen carefully, capture what you learn, then ask smarter follow-ups based on what they told you. By the end, the user should have 15-30 well-tagged memories that make every future AI interaction better.

## Before You Start

1. Run `thought_stats` to check the current state of the memory database.
2. **If the database has < 5 thoughts:** This is a fresh install. Start from Round 1.
3. **If the database has 5+ thoughts:** The user already has some context saved. Run `list_thoughts` with `limit: 10` to see what's there, then tell the user what you already know and offer to fill gaps. Skip rounds that are already well-covered.
4. Ask the user if they have a **migration export** from another AI tool they'd like to import (see "Importing Migration Data" below). If yes, handle that first — it gives you a head start and makes the interview smarter.

## The Interview

Run through these rounds in order. After each round, capture the relevant thoughts immediately — don't batch them up for later. Give the user a brief confirmation of what you saved ("Got it — saved your role, team, and current project").

Adapt your questions based on what the user tells you. If someone says "I'm a solo founder," don't ask about team structure. If they mention three active projects, dig into each one.

### Round 1: Who Are You?

Get the basics that shape every future interaction. Go beyond just the job title — understanding the whole person leads to better AI interactions.

Ask about:
- Name and what they go by
- Role and company (or if they're independent/student/hobbyist)
- Experience level — are they a senior engineer, a designer learning to code, a PM who scripts?
- What they're primarily building right now
- Where they're based (city/timezone — useful for deadline context and collaboration)
- Anything outside of work that's relevant — side projects, interests, or context they'd want AI to know about (keep this light and optional — some people want AI to know them as a whole person, others want to keep it professional)

**Capture as:**
- Type: `reference` — these are stable facts about the person
- Topics: `["identity"]` plus anything specific (e.g., `["identity", "ios", "startup"]`)
- Summary: one-line distillation (e.g., "a senior iOS engineer and solo founder building a productivity app, based in a mid-size city")

### Round 2: What Are You Working On?

Understand their active projects and goals.

Ask about:
- Active projects — names, what they do, where they are in the lifecycle
- Goals and deadlines — what's the next milestone?
- Constraints — what's blocking progress, what are they worried about?
- The tech stack for each project — languages, frameworks, infrastructure

**Capture as:**
- Type: `reference` for project descriptions, `task` for active goals with deadlines
- Topics: project names, relevant tech (e.g., `["shelby", "swift", "cloudkit"]`)
- One thought per project is ideal — don't fragment a single project across many thoughts
- If they mention deadlines, convert relative dates to absolute (e.g., "next Thursday" becomes the actual date)

### Round 3: Who Do You Work With?

Understand the people in their world (skip if they're solo).

Ask about:
- Key collaborators — names, roles, what they own
- Who makes decisions about what
- Any external stakeholders (clients, partners, contractors)

**Capture as:**
- Type: `reference`
- Topics: `["team"]` plus project names
- People: fill the `people` array with names mentioned
- Summary: role-focused (e.g., "Sarah owns backend auth, Mike handles DevOps and CI")

### Round 4: How Do You Like to Work?

This is where the AI becomes personalized — understand both work preferences and the role AI should play.

Ask about:
- **AI role** — what do they want AI to be for them? A pair programmer who thinks out loud? A research assistant who digs deep? A terse code generator? A patient teacher? This shapes every future interaction, so get it right.
- Communication style — terse or detailed? Explain reasoning or just give the answer?
- Code style — functional vs OOP? Tabs vs spaces? Framework preferences?
- Output format — do they want bullet points, prose, code-first?
- **Learning goals** — what are they actively trying to get better at? (e.g., "learning Rust," "getting better at system design," "trying to write more tests"). This helps AI tailor explanations and suggest growth opportunities.

**Capture as:**
- Type: `decision` — these are choices the user has made about how they work
- Topics: `["preferences"]` plus specifics (e.g., `["preferences", "code-style"]`, `["preferences", "ai-role"]`)
- Capture the AI role definition as its own thought — it's high-value and worth finding later
- Summary: the preference itself (e.g., "Wants AI as a senior pair programmer — think out loud, challenge assumptions")

### Round 5: What Should AI Never Do?

Anti-patterns are often more useful than preferences — they prevent the frustrations that make people abandon AI tools.

Ask about:
- Past frustrations with AI tools — what drove them crazy?
- Things AI should never assume or do unprompted
- **Corrections they've had to make repeatedly** — "I keep having to tell AI to..." is gold. These are the patterns AI gets wrong by default for this specific person.
- Any domain-specific rules (e.g., "never use Firebase," "always use snake_case")
- **Things they've changed their mind about** — "I used to do X but now I do Y." These are high-signal because they reveal how the user thinks and prevent AI from suggesting outdated approaches. (Keep this light — one or two is plenty.)

**Capture as:**
- Type: `decision`
- Topics: `["anti-patterns"]` plus context
- Summary: the rule itself (e.g., "Never add comments to code that wasn't changed")
- For changed-mind items, capture both the old and new position — this prevents AI from accidentally recommending the abandoned approach
- These are high-value — be specific and preserve the user's exact framing

### Round 6: Anything Else?

Open-ended — let the user share whatever they think is important.

Ask:
- "Is there anything else you'd want a new team member to know on their first day?"
- "Anything I missed that would help me help you better?"

**Capture** with whatever type fits best.

## Importing Migration Data

If the user has output from the migration prompt (from `advisaur-mcp migrate`), they'll paste a structured block of text. Parse it and capture each item as a thought:

1. Read through the entire pasted content first to understand the structure
2. For each distinct piece of information, create a thought with:
   - Appropriate `type` (match to `reference`, `decision`, `insight`, `task`, or `note`)
   - A concise `summary` (under 100 characters)
   - Relevant `topics` and `people` arrays
   - `source` set to `"migration"`
3. After importing, run `search_thoughts` on a few key terms to verify the import worked
4. Tell the user how many memories were imported and what topics they cover
5. Use the imported data to skip interview rounds that are already well-covered — jump to filling gaps

Don't duplicate information that's already in the database. If the migration data overlaps with existing memories, update the existing ones rather than creating duplicates.

## Wrapping Up

After the final round:

1. Run `thought_stats` to show the user the final count
2. Give a brief summary of what was captured — group by area (identity, projects, team, preferences, anti-patterns)
3. Suggest connecting related thoughts: "I can run a quick pass to link related memories — want me to?" If yes, use `manage_edges` to create `related` edges between thoughts that reference the same projects, people, or topics.
4. Mention Forage: "Advisaur MCP has a daily maintenance skill called Forage that will automatically enrich and connect your memories over time. If you haven't set it up yet, run `advisaur-mcp setup <your-agent> --forage`."
5. End with something like: "You're all set. Your AI tools now have context about who you are, what you're building, and how you like to work. This will get better over time as more memories are captured naturally."

## Guidelines

- **Be conversational.** This should feel like a first meeting with a thoughtful colleague, not a government form. Adapt your tone to the user — if they're casual, be casual. If they're precise, be precise.
- **Capture after each round.** The user should see memories being saved in real time. This makes the value tangible and builds trust in the tool.
- **Quality over quantity.** 20 well-structured memories beat 50 vague ones. Each thought should have a good summary, accurate type, and relevant topics.
- **Don't over-ask.** If the user gives short answers, take the hint. Not everyone has a large team or complex preferences. 3 excellent rounds are better than 6 awkward ones.
- **Respect boundaries.** If the user doesn't want to share something, move on. This is about making AI useful, not a personnel file.
- **Use `source: "onboard"`** on all thoughts captured during this session so they're identifiable later.
