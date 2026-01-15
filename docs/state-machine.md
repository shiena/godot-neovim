# Godot Neovim State Machine

## Mode Transitions

```mermaid
stateDiagram-v2
    [*] --> Normal: 起動時

    %% Normal mode transitions
    Normal --> Insert: i, a, o, O, A, I, c{motion}, s, C, S
    Normal --> Replace: R
    Normal --> Visual: v
    Normal --> VisualLine: V
    Normal --> VisualBlock: Ctrl+V, Ctrl+B
    Normal --> Command: :
    Normal --> Search: /, ?
    Normal --> OperatorPending: d, c, y, >, <, =, gq, etc.
    Normal --> PendingCharOp: f, F, t, T, r

    %% Operator-pending state
    OperatorPending --> Normal: {motion}, Escape
    OperatorPending --> Insert: c{motion}完了時

    %% Pending char op state
    PendingCharOp --> Normal: {char}, Escape

    %% Insert mode transitions
    Insert --> Normal: Escape, Ctrl+[
    Insert --> Normal: (Ctrl+B step 1)
    Normal --> VisualBlock: (Ctrl+B step 2)
    note right of Insert: Ctrl+B は内部的に\nNormal経由でVisualBlockへ

    %% Replace mode transitions
    Replace --> Normal: Escape, Ctrl+[

    %% Visual mode transitions
    Visual --> Normal: Escape, v, d, y, x, p
    Visual --> VisualLine: V
    Visual --> VisualBlock: Ctrl+V, Ctrl+B
    Visual --> Insert: c, s, C, S

    %% Visual Line mode transitions
    VisualLine --> Normal: Escape, V, d, y, x, p
    VisualLine --> Visual: v
    VisualLine --> VisualBlock: Ctrl+V, Ctrl+B
    VisualLine --> Insert: c, s, C, S

    %% Visual Block mode transitions
    VisualBlock --> Normal: Escape, Ctrl+V, Ctrl+B, d, y, x, p
    VisualBlock --> Visual: v
    VisualBlock --> VisualLine: V
    VisualBlock --> Insert: c, s, C, S, I, A

    %% Command mode transitions
    Command --> Normal: Enter, Escape

    %% Search mode transitions
    Search --> Normal: Enter, Escape
```

## Simplified View (主要な遷移のみ)

```mermaid
stateDiagram-v2
    [*] --> Normal

    Normal --> Insert: i, a, o, c, s
    Normal --> Replace: R
    Normal --> Visual: v, V, Ctrl+V
    Normal --> Command: :
    Normal --> Search: /, ?

    Insert --> Normal: Esc
    Replace --> Normal: Esc
    Visual --> Normal: Esc, 操作完了
    Visual --> Insert: c, s
    Command --> Normal: Enter, Esc
    Search --> Normal: Enter, Esc
```

## Mode Details

| Mode | Display | Color | Cursor | 内部状態 |
|------|---------|-------|--------|----------|
| Normal | NORMAL | Green | Block | `current_mode = "n"` |
| Insert | INSERT | Blue | Line | `current_mode = "i"` |
| Replace | REPLACE | Red | Line | `current_mode = "R"` |
| Visual | VISUAL | Orange | Block | `current_mode = "v"` |
| VisualLine | V-LINE | Orange | Block | `current_mode = "V"` |
| VisualBlock | V-BLOCK | Orange | Block | `current_mode = "\x16"` |
| Command | : | Yellow | - | `command_mode = true` |
| Search | / or ? | Yellow | - | `search_mode = true` |
| OperatorPending | (表示なし) | - | - | `blocking = true` |
| PendingCharOp | (表示なし) | - | - | `pending_char_op = Some(...)` |

## Input Flow

```mermaid
flowchart TD
    A[Godot Input Event] --> B{command_mode?}
    B -->|Yes| C[handle_command_mode_input]
    B -->|No| D{search_mode?}
    D -->|Yes| E[handle_search_mode_input]
    D -->|No| F{pending_char_op?}
    F -->|Yes| G[handle_pending_char_op]
    F -->|No| H{insert_mode?}
    H -->|Yes| I[handle_insert_mode_input]
    H -->|No| J{replace_mode?}
    J -->|Yes| K[handle_replace_mode_input]
    J -->|No| L[handle_normal_mode_input]

    C -->|Enter| M[Execute command]
    C -->|Escape| N[Close command line]
    E -->|Enter| O[Execute search via Neovim]
    E -->|Escape| P[Cancel search]
    G --> Q[Send combined key to Neovim]
    I --> R[send_keys to Neovim]
    K --> R
    L --> R

    M --> S[Update UI]
    N --> S
    O --> S
    P --> S
    Q --> S
    R --> T[Neovim processes]
    T --> U[get_mode / Redraw events]
    U --> V{blocking?}
    V -->|Yes| W[Operator pending - skip sync]
    V -->|No| X[Update current_mode]
    W --> S
    X --> S
```

## Buffer Sync Flow

```mermaid
sequenceDiagram
    participant G as Godot Editor
    participant P as Plugin
    participant N as Neovim

    Note over G,N: Insert Mode Exit (Escape)
    G->>P: Escape key
    P->>N: input("<Esc>")
    P->>N: sync_buffer_to_neovim()
    P->>N: sync_cursor_to_neovim()
    P->>P: current_mode = "n"
    P->>G: Update mode display (NORMAL)

    Note over G,N: Normal Mode Command (e.g., "dd")
    G->>P: Key event "d"
    P->>N: send_keys("d")
    N->>P: blocking = true (operator pending)
    P->>P: Skip buffer sync
    G->>P: Key event "d"
    P->>N: send_keys("d")
    N->>P: blocking = false, cursor, buffer
    P->>P: Update current_mode
    P->>G: sync_buffer_from_neovim()
    P->>G: Update cursor position

    Note over G,N: Pending Char Op (e.g., "fa")
    G->>P: Key event "f"
    P->>P: pending_char_op = Some('f')
    G->>P: Key event "a"
    P->>N: send_keys("fa")
    P->>P: pending_char_op = None
    N->>P: cursor position
    P->>G: Update cursor
```

## Ctrl+B Special Behavior

```mermaid
flowchart LR
    subgraph "Insert Mode"
        I[Insert] -->|Ctrl+B| I1[send_escape]
        I1 --> I2[Normal]
        I2 -->|send_keys Ctrl+V| I3[VisualBlock]
    end

    subgraph "Normal Mode"
        N[Normal] -->|Ctrl+B| N1[page_up]
    end

    subgraph "Visual Modes"
        V[Visual/V-Line] -->|Ctrl+B| V1[VisualBlock]
    end
```

Note: `Ctrl+B` は Godot が `Ctrl+V` をインターセプトするため、VisualBlock への代替キーとして実装されている。
