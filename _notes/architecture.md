# Architecture: DuckDB Semantic Views Extension

This document describes the internal architecture of the `duckdb-semantic-views` extension as of v0.5.4. The codebase spans 14,578 lines across 16 Rust source files and 1 C++ shim (297 lines), with 468 tests (376 unit + 36 integration + 44 property + 6 proptests + 5 output + 1 doc). The extension implements a declarative semantic layer for DuckDB: users define dimensions, metrics, relationships, facts, and hierarchies via `CREATE SEMANTIC VIEW` DDL, then query them via a `semantic_view()` table function that expands to SQL at runtime.

## 1. High-Level Component Overview

```mermaid
flowchart TD
    subgraph User["User SQL"]
        DDL["CREATE SEMANTIC VIEW ..."]
        Query["semantic_view('v', dimensions := [...], metrics := [...])"]
    end

    subgraph Parser["Parser Layer"]
        CPP["C++ shim.cpp<br/>ParserExtension hooks"]
        FFI["Rust FFI<br/>parse.rs + body_parser.rs"]
    end

    subgraph Model["Semantic Model"]
        MOD["model.rs<br/>SemanticViewDefinition"]
        GRP["graph.rs<br/>RelationshipGraph + validation"]
    end

    subgraph Engine["Query Engine"]
        EXP["expand.rs<br/>SQL expansion pipeline"]
    end

    subgraph Storage["Storage"]
        CAT["catalog.rs<br/>Arc&lt;RwLock&lt;HashMap&gt;&gt;<br/>+ semantic_layer._definitions"]
    end

    subgraph Exec["Execution"]
        DEF["ddl/define.rs"]
        DRP["ddl/drop.rs"]
        LST["ddl/list.rs + describe.rs"]
        TBL["query/table_function.rs"]
        EXPL["query/explain.rs"]
        ERR["query/error.rs"]
    end

    DDL --> CPP --> FFI --> MOD
    FFI -->|rewritten SQL| CPP
    CPP -->|ddl_conn| DEF
    DEF --> GRP
    DEF --> CAT

    Query --> TBL --> CAT
    TBL --> EXP --> GRP
    EXP --> MOD

    DRP --> CAT
    LST --> CAT
    EXPL --> EXP

    style User fill:#e8f4fd,stroke:#2196f3
    style Parser fill:#fff3e0,stroke:#ff9800
    style Model fill:#e8f5e9,stroke:#4caf50
    style Engine fill:#fce4ec,stroke:#e91e63
    style Storage fill:#f3e5f5,stroke:#9c27b0
    style Exec fill:#e0f2f1,stroke:#009688
```

**Cross-cutting: Three DuckDB connections** (`persist_conn`, `query_conn`, `ddl_conn`) work around the non-reentrant `ClientContext` lock. See [Section 6](#6-connection-management).

## 2. Module Map with Line Counts

```mermaid
flowchart LR
    subgraph core["Core Modules"]
        model["model.rs<br/>836 lines"]
        catalog["catalog.rs<br/>473 lines"]
        parse["parse.rs<br/>1,518 lines"]
        body["body_parser.rs<br/>2,107 lines"]
        grph["graph.rs<br/>2,502 lines"]
        expand["expand.rs<br/>4,490 lines"]
    end

    subgraph ddl["DDL (ddl/)"]
        define["define.rs<br/>229 lines"]
        drop["drop.rs<br/>139 lines"]
        list["list.rs<br/>101 lines"]
        describe["describe.rs<br/>164 lines"]
    end

    subgraph query["Query (query/)"]
        table_fn["table_function.rs<br/>810 lines"]
        explain["explain.rs<br/>260 lines"]
        error["error.rs<br/>108 lines"]
    end

    subgraph other["Entry + Shim"]
        lib["lib.rs<br/>535 lines"]
        shim["shim.cpp<br/>297 lines"]
    end

    %% Dependency arrows (use crate::)
    parse --> body
    parse --> model
    body --> model
    body -.->|ParseError| parse
    catalog --> model
    grph --> model
    grph -.->|suggest_closest| expand
    expand --> grph
    expand --> model
    define --> catalog
    drop --> catalog
    list --> catalog
    describe --> catalog
    table_fn --> catalog
    table_fn --> expand
    table_fn --> model
    explain --> expand
    explain --> model
    error --> expand

    %% C++ -> Rust FFI
    shim -.->|FFI| parse

    linkStyle 3 stroke:#f44336,stroke-dasharray:5
    linkStyle 5 stroke:#f44336,stroke-dasharray:5
```

**Circular dependency:** `graph.rs` imports `expand::suggest_closest` and `expand.rs` imports `graph::RelationshipGraph`. The `suggest_closest` function is a generic Levenshtein utility that has no semantic dependency on the expansion engine.

## 3. DDL Flow -- CREATE SEMANTIC VIEW

The DDL path spans two languages and three phases: parse, plan, and bind/execute.

```mermaid
sequenceDiagram
    participant User
    participant DuckDB as DuckDB Parser
    participant CPP as C++ shim.cpp
    participant Rust as Rust parse.rs
    participant Body as body_parser.rs
    participant Plan as sv_plan_function
    participant Bind as sv_ddl_bind
    participant Rewrite as sv_rewrite_ddl_rust
    participant DDL as ddl_conn
    participant Define as DefineFromJsonVTab
    participant Graph as graph.rs
    participant Cat as catalog.rs

    User->>DuckDB: CREATE SEMANTIC VIEW sales AS ...
    DuckDB->>DuckDB: Standard parser fails
    DuckDB->>CPP: sv_parse_stub(query)
    CPP->>Rust: sv_validate_ddl_rust(query)
    Rust->>Rust: detect_ddl_prefix()
    Rust->>Body: parse_keyword_body()
    Body-->>Rust: Ok(SemanticViewDefinition)
    Rust-->>CPP: rc=0 (PARSE_SUCCESSFUL)
    CPP-->>DuckDB: SemanticViewParseData{query}

    DuckDB->>Plan: sv_plan_function(parse_data)
    Plan-->>DuckDB: TableFunction{sv_ddl_bind, query}

    DuckDB->>Bind: sv_ddl_bind(query)
    Bind->>Rewrite: sv_rewrite_ddl_rust(query)
    Rewrite->>Body: parse_keyword_body()
    Body-->>Rewrite: SemanticViewDefinition as JSON
    Rewrite-->>Bind: SELECT * FROM create_semantic_view_from_json('sales', '{...}')
    Bind->>DDL: duckdb_query(ddl_conn, rewritten_sql)
    DDL->>Define: DefineFromJsonVTab::bind(name, json)
    Define->>Graph: validate_graph()
    Define->>Graph: validate_facts()
    Define->>Graph: validate_hierarchies()
    Define->>Graph: validate_derived_metrics()
    Define->>Graph: validate_using_relationships()
    Define->>Cat: catalog_insert / catalog_upsert
    Define-->>DDL: Ok(view_name)
    DDL-->>Bind: result rows
    Bind-->>DuckDB: SvDdlBindData{rows}
    DuckDB-->>User: view_name
```

Key observations:
- **Double parse**: Both `sv_validate_ddl_rust` (parse phase) and `sv_rewrite_ddl_rust` (plan/bind phase) call `body_parser::parse_keyword_body`. This is intentional -- no state carries between the C++ parse and plan callbacks, and DDL is infrequent enough that the cost is negligible.
- **Error positioning**: The validate path tracks byte offsets through parsing so the C++ shim can set `error_location` on `ParserExtensionParseResult`, giving users caret-positioned error messages.

## 4. Query Flow -- semantic_view()

```mermaid
sequenceDiagram
    participant User
    participant DuckDB
    participant VTab as SemanticViewVTab
    participant Cat as catalog.rs
    participant Exp as expand()
    participant Graph as RelationshipGraph
    participant QConn as query_conn
    participant Stream as Zero-copy streaming

    User->>DuckDB: SELECT * FROM semantic_view('sales', dimensions := ['region'], metrics := ['revenue'])
    DuckDB->>VTab: bind(view_name, dimensions, metrics)
    VTab->>Cat: catalog.read().get("sales")
    Cat-->>VTab: JSON definition
    VTab->>VTab: SemanticViewDefinition::from_json()
    VTab->>Exp: expand("sales", def, QueryRequest)
    Exp->>Exp: validate request
    Exp->>Exp: resolve dimensions + metrics
    Exp->>Exp: toposort_facts + inline_facts
    Exp->>Exp: toposort_derived + inline_derived_metrics
    Exp->>Exp: check_fan_traps
    Exp->>Exp: find_using_context (role-playing)
    Exp->>Graph: RelationshipGraph::from_definition
    Exp->>Exp: resolve_joins_pkfk
    Exp->>Exp: SQL assembly (SELECT/FROM/JOIN/WHERE/GROUP BY)
    Exp-->>VTab: expanded SQL string
    VTab->>VTab: build_execution_sql (type cast wrapper)
    VTab->>VTab: Declare output columns + types
    VTab-->>DuckDB: SemanticViewBindData

    DuckDB->>VTab: func(output_chunk)
    VTab->>QConn: duckdb_query(query_conn, execution_sql)
    QConn-->>VTab: duckdb_result
    VTab->>Stream: duckdb_result_get_chunk()
    Stream->>Stream: duckdb_vector_reference_vector (zero-copy)
    VTab-->>DuckDB: DataChunkHandle with output
```

The `build_execution_sql` step wraps the expanded SQL in a subquery with explicit `CAST` expressions when bind-time types differ from the inferred types (e.g., DuckDB's optimizer may promote `BIGINT` to `HUGEINT`).

## 5. Connection Management

```mermaid
flowchart LR
    subgraph DB["DuckDB Database"]
        main["Main Connection<br/>(host)"]
        persist["persist_conn<br/>(file-backed only)"]
        query["query_conn"]
        ddl["ddl_conn"]
    end

    subgraph Users["Used By"]
        init["init_extension()<br/>registers all table functions"]
        cat_write["catalog persist<br/>(INSERT into _definitions)"]
        sem_query["semantic_view() func()<br/>executes expanded SQL"]
        ddl_exec["sv_ddl_bind()<br/>executes rewritten DDL"]
    end

    main --> init
    persist --> cat_write
    query --> sem_query
    ddl --> ddl_exec

    style persist fill:#fff9c4,stroke:#f9a825
    style query fill:#e8f5e9,stroke:#4caf50
    style ddl fill:#fce4ec,stroke:#e91e63
```

| Connection | Created | Purpose | Why separate? |
|---|---|---|---|
| **main** (host) | By DuckDB | Registers table functions, reads catalog at init | -- |
| **persist_conn** | At init (file-backed only) | Writes to `semantic_layer._definitions` | ClientContext lock is non-reentrant; `bind()` already holds it |
| **query_conn** | At init (always) | Executes expanded SQL in `semantic_view()` `func()` | Same lock reason; query runs inside `func()` callback |
| **ddl_conn** | At init (always) | Executes rewritten DDL SQL in `sv_ddl_bind()` | Plan/bind holds ClientContext; DDL rewrite needs a second execution context |

## 6. Data Model

```mermaid
classDiagram
    class SemanticViewDefinition {
        +String base_table
        +Vec~TableRef~ tables
        +Vec~Dimension~ dimensions
        +Vec~Metric~ metrics
        +Vec~String~ filters
        +Vec~Join~ joins
        +Vec~Fact~ facts
        +Vec~Hierarchy~ hierarchies
        +Vec~String~ column_type_names
        +Vec~u32~ column_types_inferred
        +from_json(name, json) Result
    }

    class TableRef {
        +String alias
        +String table
        +Vec~String~ pk_columns
        +Vec~Vec~String~~ unique_constraints
    }

    class Dimension {
        +String name
        +String expr
        +Option~String~ source_table
        +Option~String~ output_type
    }

    class Metric {
        +String name
        +String expr
        +Option~String~ source_table
        +Option~String~ output_type
        +Vec~String~ using_relationships
    }

    class Fact {
        +String name
        +String expr
        +Option~String~ source_table
    }

    class Hierarchy {
        +String name
        +Vec~String~ levels
    }

    class Join {
        +String table
        +String on
        +Vec~String~ from_cols
        +Vec~JoinColumn~ join_columns
        +String from_alias
        +Vec~String~ fk_columns
        +Vec~String~ ref_columns
        +Option~String~ name
        +Cardinality cardinality
    }

    class JoinColumn {
        +String from
        +String to
    }

    class Cardinality {
        <<enumeration>>
        ManyToOne
        OneToOne
    }

    SemanticViewDefinition *-- TableRef
    SemanticViewDefinition *-- Dimension
    SemanticViewDefinition *-- Metric
    SemanticViewDefinition *-- Fact
    SemanticViewDefinition *-- Hierarchy
    SemanticViewDefinition *-- Join
    Join *-- JoinColumn
    Join --> Cardinality
```

All types derive `Serialize`/`Deserialize` (serde) and `Arbitrary` (proptest). The `Join` struct carries legacy fields (`on`, `from_cols`, `join_columns`) alongside the current PK/FK model (`from_alias`, `fk_columns`, `ref_columns`, `name`, `cardinality`) for backward-compatible deserialization of stored JSON.

## 7. Expansion Pipeline Internals

The `expand()` function (line 1240 of `expand.rs`) implements a 9-step pipeline:

```mermaid
flowchart TD
    A["1. Validate request<br/>(at least one dim or metric)"] --> B
    B["2. Resolve dimensions<br/>(find_dimension, duplicate check)"] --> C
    C["3. Resolve metrics<br/>(find_metric, duplicate check)"] --> D
    D["4a. Toposort facts<br/>(toposort_facts)"] --> E
    E["4b. Inline facts + derived metrics<br/>(inline_derived_metrics)"] --> F
    F["5. Fan trap detection<br/>(check_fan_traps)"] --> G
    G["6. Role-playing resolution<br/>(find_using_context → dim_scoped_aliases)"] --> H
    H["7. Build SELECT clause<br/>(dims → DISTINCT, mets → GROUP BY)"] --> I
    I["8. Resolve joins<br/>(resolve_joins_pkfk → ordered LEFT JOINs)"] --> J
    J["9. SQL assembly<br/>(FROM + JOINs + WHERE + GROUP BY)"]

    style A fill:#e3f2fd,stroke:#1565c0
    style F fill:#ffebee,stroke:#c62828
    style G fill:#fff3e0,stroke:#e65100
    style I fill:#e8f5e9,stroke:#2e7d32
```

Supporting functions called by the pipeline:

| Step | Function | Lines | Purpose |
|---|---|---|---|
| 4a | `toposort_facts()` | 673-750 | Topological sort of fact dependency DAG |
| 4b | `inline_facts()` | 752-802 | Word-boundary-safe substitution of fact refs in expressions |
| 4b | `toposort_derived()` | 804-883 | Topological sort of derived metric DAG |
| 4b | `inline_derived_metrics()` | 885-1008 | Resolves all metric expressions (base facts + derived refs) |
| 5 | `check_fan_traps()` | 1010-1237 | Detects one-to-many aggregation across relationship boundaries |
| 6 | `find_using_context()` | (inline) | Resolves role-playing table aliases via USING RELATIONSHIPS |
| 8 | `resolve_joins_pkfk()` | 487-671 | BFS from base table to collect required join aliases |
| 8 | `synthesize_on_clause()` | 278-290 | Generates ON clause from FK/PK column pairs |

## 8. Architectural Critique

### C1: expand.rs is a monolith (4,490 lines)

**Severity: High | Effort: Large**

`expand.rs` contains 6 distinct responsibilities in a single file: request validation, dimension/metric resolution, fact topological sort and inlining, derived metric resolution, fan trap detection, role-playing resolution, join graph resolution, and SQL generation. At 4,490 lines (99 tests), it is the largest file by a factor of 1.8x over the next (`graph.rs` at 2,502).

**Proposed refactoring:** Split into an `expand/` module directory:

```
src/expand/
  mod.rs          - pub fn expand(), QueryRequest, ExpandError (public API)
  validate.rs     - request validation, duplicate checks
  resolve.rs      - find_dimension, find_metric, dimension/metric resolution
  facts.rs        - toposort_facts, inline_facts, toposort_derived, inline_derived_metrics
  fan_trap.rs     - check_fan_traps
  role_playing.rs - find_using_context, dim_scoped_aliases
  join_resolver.rs - resolve_joins_pkfk, synthesize_on_clause
  sql_gen.rs      - SELECT/FROM/JOIN/WHERE/GROUP BY assembly, quote_ident, quote_table_ref
  util.rs         - suggest_closest, replace_word_boundary
```

### C2: graph.rs validates 6 different concerns (2,502 lines)

**Severity: Medium | Effort: Medium**

`graph.rs` spans relationship graph construction, FK validation, fact validation, hierarchy validation, derived metric validation, and USING relationship validation. Each `validate_*` function is self-contained and could be a separate module.

**Proposed refactoring:** Split into a `graph/` module directory:

```
src/graph/
  mod.rs               - RelationshipGraph struct + from_definition
  relationship.rs      - validate_graph, toposort, check_no_diamonds, check_no_orphans
  facts.rs             - validate_facts, find_fact_references
  hierarchies.rs       - validate_hierarchies
  derived_metrics.rs   - validate_derived_metrics, contains_aggregate_function
  using.rs             - validate_using_relationships
```

### C3: Circular dependency expand <-> graph

**Severity: Medium | Effort: Small**

`expand.rs` exports `suggest_closest` (a generic Levenshtein distance utility) which `graph.rs` imports. Meanwhile, `expand.rs` imports `graph::RelationshipGraph`. This creates a circular dependency at the module level.

`suggest_closest` has zero semantic connection to query expansion -- it's a string similarity helper used for "did you mean?" suggestions in error messages.

**Proposed fix:** Extract `suggest_closest` and `replace_word_boundary` to a new `src/util.rs` module. Both `expand.rs` and `graph.rs` import from `util` instead of each other. This breaks the cycle and makes the dependency graph a clean DAG.

### C4: Double parse in FFI boundary

**Severity: Low | Effort: N/A (intentional)**

Both `sv_validate_ddl_rust` (parse phase) and `sv_rewrite_ddl_rust` (plan/bind phase) call `body_parser::parse_keyword_body`. The DuckDB parser extension API provides no mechanism to carry state between parse and plan callbacks -- the C++ `ParserExtensionParseData` only carries the raw query string.

Since DDL statements are infrequent (once at view creation), the cost of re-parsing is negligible. Not worth optimizing.

### C5: parse.rs <-> body_parser.rs bidirectional import

**Severity: Low | Effort: Small**

`parse.rs` imports `body_parser::parse_keyword_body`. `body_parser.rs` imports `parse::ParseError`. This bidirectional coupling exists because `ParseError` is defined in `parse.rs` but is the error type returned by `body_parser`.

**Proposed fix:** Extract `ParseError` to a shared `error.rs` or to `model.rs`. Both `parse.rs` and `body_parser.rs` import from the shared location.

### C6: Proposed refactored dependency graph

After applying C1 (split expand), C2 (split graph), C3 (extract util), and C5 (shared error):

```mermaid
flowchart TD
    subgraph core["Core"]
        model["model.rs"]
        util["util.rs<br/>(NEW)"]
        error["errors.rs<br/>(NEW)"]
    end

    subgraph parser["Parser"]
        parse["parse.rs"]
        body["body_parser.rs"]
    end

    subgraph graph_mod["graph/"]
        graph_rel["relationship.rs"]
        graph_facts["facts.rs"]
        graph_hier["hierarchies.rs"]
        graph_derived["derived_metrics.rs"]
        graph_using["using.rs"]
    end

    subgraph expand_mod["expand/"]
        exp_mod["mod.rs"]
        exp_validate["validate.rs"]
        exp_resolve["resolve.rs"]
        exp_facts["facts.rs"]
        exp_fan["fan_trap.rs"]
        exp_role["role_playing.rs"]
        exp_join["join_resolver.rs"]
        exp_sql["sql_gen.rs"]
    end

    subgraph storage["Storage"]
        catalog["catalog.rs"]
    end

    subgraph ddl["ddl/"]
        define["define.rs"]
        drop["drop.rs"]
        list["list.rs"]
        describe["describe.rs"]
    end

    subgraph query["query/"]
        table_fn["table_function.rs"]
        explain["explain.rs"]
        qerr["error.rs"]
    end

    parse --> body
    parse --> model
    body --> model
    body --> error
    parse --> error

    graph_rel --> model
    graph_rel --> util
    graph_facts --> model
    graph_facts --> util
    graph_hier --> model
    graph_derived --> model
    graph_using --> model

    exp_mod --> model
    exp_validate --> model
    exp_resolve --> model
    exp_resolve --> util
    exp_facts --> model
    exp_fan --> model
    exp_fan --> graph_rel
    exp_role --> model
    exp_join --> model
    exp_join --> graph_rel
    exp_sql --> model

    catalog --> model
    define --> catalog
    drop --> catalog
    list --> catalog
    describe --> catalog
    table_fn --> catalog
    table_fn --> exp_mod
    table_fn --> model
    explain --> exp_mod
    explain --> model
    qerr --> exp_mod

    style core fill:#e8f5e9,stroke:#4caf50
    style expand_mod fill:#fce4ec,stroke:#e91e63
    style graph_mod fill:#e3f2fd,stroke:#1565c0
```

Key improvements:
- **No circular dependencies** -- all arrows flow downward
- **Single-responsibility modules** -- each file has one concern
- **util.rs breaks the expand<->graph cycle** -- string utilities are shared infrastructure
- **errors.rs centralizes ParseError** -- eliminates parse<->body_parser bidirectional import
- **expand/ and graph/ are module directories** -- internal structure is navigable without reading 4,000+ line files
