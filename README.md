# judge

A terminal-based interview panel tool for technical interviewers. Capture timestamped notes, rate candidates across key dimensions, and generate structured feedback reports — all without leaving your terminal.

## Install

```sh
cargo install judge
```

## Usage

### Start an interview

```sh
judge new "Alice Chen" --role "Senior Rust Engineer" --interviewer "Hamza"
```

```
✓ Interview started for Alice Chen
  ID:   beeeb896
  Role: Senior Rust Engineer
```

### Add notes during the interview

```sh
judge note beeeb896 "Solved binary tree inversion in 8 min, clean approach"
judge note beeeb896 "Struggled to explain time complexity" --tag concern
```

### Rate the candidate (1–10)

```sh
judge rate beeeb896 technical 8
judge rate beeeb896 problem-solving 7
judge rate beeeb896 communication 6
```

Suggested categories: `technical`, `problem-solving`, `communication`, `culture`

### Close with a verdict

```sh
judge close beeeb896 hire --summary "Strong fundamentals, needs growth on communication"
```

Verdicts: `hire`, `no-hire`, `strong-hire`, `strong-no-hire`

### Generate a report

```sh
judge report beeeb896
```

```
════════════════════════════════════════════════════════════
  INTERVIEW REPORT
════════════════════════════════════════════════════════════
  Candidate:   Alice Chen
  Role:        Senior Rust Engineer
  Interviewer: Hamza
  Date:        April 22, 2026
  Started:     11:15
  Ended:       11:32 (17 min)

  RATINGS
  ────────────────────────────────────────────────
  communication            6/10  ██████░░░░
  problem-solving          7/10  ███████░░░
  technical                8/10  ████████░░

  Average Score           7.0/10

  NOTES
  ────────────────────────────────────────────────
  [11:18] Solved binary tree inversion in 8 min, clean approach
  [11:25] [CONCERN] Struggled to explain time complexity

  VERDICT
  ────────────────────────────────────────────────
  Hire

  Strong fundamentals, needs growth on communication
════════════════════════════════════════════════════════════
```

### Other commands

```sh
judge list           # list all interviews
judge list --active  # only active sessions
judge show <id>      # quick summary view
judge delete <id>    # delete an interview
```

IDs support prefix matching — type just enough characters to be unique.

## Data storage

Interviews are saved to `~/.local/share/judge/interviews.json`.

## License

MIT
