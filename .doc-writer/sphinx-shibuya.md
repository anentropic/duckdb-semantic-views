# Sphinx Shibuya Guidance

Opinionated preferences for writing documentation with Sphinx and the Shibuya theme. These are style preferences -- agents already understand RST and Sphinx syntax. This file guides how features should be used, not what they are.

## Admonitions

- Prefer admonition directives over bold text for callouts. Bold text in paragraphs gets lost; directives create visual hierarchy that readers scan.
- Use specific admonition types instead of generic `.. note::`:
  - `.. tip::` for best practices and recommended approaches
  - `.. warning::` for gotchas, common mistakes, and things that might surprise
  - `.. danger::` for breaking changes, security concerns, and data loss risks
  - `.. note::` for supplementary context that enriches understanding
  - `.. versionadded:: X.Y` for new features introduced in a specific release
  - `.. deprecated:: X.Y` for deprecation notices with migration guidance
- Do not stack multiple admonitions back-to-back. If you need three warnings in a row, consolidate into one admonition with a list.
- Example:

  ```rst
  .. tip::

     Use ``client.connect(timeout=30)`` for slow networks. The default
     timeout of 10 seconds is too aggressive for high-latency connections.
  ```

## Code Blocks

- Always use `.. code-block:: {language}` -- never use bare `::` for code that readers will copy. Bare `::` omits syntax highlighting and language context.
- Use `:emphasize-lines:` to highlight important lines in longer examples. Do not highlight everything -- highlight only the lines that are new or critical.
- Use `:caption:` to label file paths: `.. code-block:: python` with `:caption: src/main.py`. This tells readers where the code belongs.
- Use `:linenos:` only for long examples where line numbers aid discussion in surrounding text. Short snippets do not need line numbers.
- Prefer tab sets (sphinx-design) over consecutive code blocks when showing the same concept in multiple languages or installation methods.

## Tab Sets

- Use `.. tab-set::` with `.. tab-item::` directives from sphinx-design for multi-language or multi-method examples. Tabs keep alternatives side-by-side without page bloat.
- Use `:sync-group:` on the tab-set and `:sync:` on tab-items to synchronize related tabs across the page. When a reader selects Python in one tab set, all tab sets with the same sync group switch to Python.
- Keep tab labels short and consistent: "Python", "JavaScript", "pip", "conda" -- not "Python Example" or "Install via pip".
- Group related alternatives: install methods (pip/conda/docker), OS-specific instructions (macOS/Linux/Windows), sync/async patterns.
- Do not use tabs for unrelated content. If the tabs show different concepts rather than different expressions of the same concept, use separate sections instead.
- Example:

  ```rst
  .. tab-set::
     :sync-group: lang

     .. tab-item:: Python
        :sync: python

        .. code-block:: python

           client = MyClient(api_key="...")

     .. tab-item:: JavaScript
        :sync: js

        .. code-block:: javascript

           const client = new MyClient({ apiKey: "..." });
  ```

## sphinx-design Components

sphinx-design provides grids, cards, dropdowns, badges, and buttons as RST directives. Use them proactively where they improve clarity or navigation -- do not wait for the user to ask.

- **Grids and cards** (`.. grid::`, `.. grid-item-card::`): Use for landing pages, feature overviews, and section index pages. Grid responsiveness values `1 2 3 3` adapt from 1 column on mobile to 3 on desktop. Use `:link:` on cards for navigation so the entire card is clickable.
- **Dropdowns** (`.. dropdown::`): Use for optional or advanced content that most readers can skip. Equivalent to collapsible admonitions in other frameworks. Use `:color:` and `:icon:` for visual distinction between informational and warning dropdowns.
- **Badges** (`:bdg-success:`, `:bdg-warning:`, `:bdg-info:`): Use inline for version markers (`v2.1`), stability status (`stable`, `beta`), and deprecation notices. Keep badge text short -- one or two words.
- **Buttons** (`.. button-ref::`): Use on landing pages for prominent calls-to-action (e.g., "Get Started" pointing to the quickstart tutorial). Do not overuse -- one or two buttons per page maximum.
- Be proactive about suggesting sphinx-design features where they improve clarity or navigation. Suggest grids for landing pages, tabs for multi-language code, dropdowns for advanced content.
- Reference: full component inventory at https://sphinx-design.readthedocs.io/en/latest/

## Navigation

### Three Navigation Strategies

The setup Q&A offers three strategies. The guidance below documents how each renders in Sphinx/Shibuya so the doc-author writes the correct structure.

- The front page must appear as "Overview" in the nav at the same level as the section tabs — use "Overview" rather than "Home", as it signals that the page orients the reader to the project structure.
- For sidebar-only nav (no nav_links), ensure the root index.rst is labelled "Overview" as the first sidebar entry, at the same level as the section headings.

#### Strategy 1: Sections as Top Tabs

Each diataxis section (Tutorials, How-To Guides, Explanation, Reference) becomes a dropdown tab in the top navbar via `nav_links` in conf.py. Each dropdown shows children with summaries.

- `nav_links` are configured in `html_theme_options` in conf.py. They are static URL lists -- they do NOT derive from the toctree and have no build-time validation.
- Each section has its own `index.rst` with a `:hidden:` toctree listing its child pages.
- The root `index.rst` has a hidden toctree listing all section index files.
- When a reader clicks a tab, the sidebar shows only that section's toctree (scoped sidebar).
- nav_links URLs are plain strings (e.g., `"tutorials/index"`), NOT RST cross-references. Do NOT use `:ref:` or `:doc:` syntax in nav_links.

Example conf.py nav_links for sections-as-tabs:

```python
"nav_links": [
    {
        "title": "Tutorials",
        "url": "tutorials/index",
        "children": [
            {
                "title": "Getting Started",
                "url": "tutorials/getting-started",
                "summary": "Install and run your first example"
            },
        ],
    },
    {
        "title": "How-To Guides",
        "url": "how-to/index",
        "children": [
            {
                "title": "Authentication",
                "url": "how-to/authentication",
                "summary": "Configure OAuth, API keys, and token refresh"
            },
        ],
    },
    {
        "title": "Explanation",
        "url": "explanation/index",
        "children": [
            {
                "title": "Architecture Overview",
                "url": "explanation/architecture",
                "summary": "How the library is structured and why"
            },
        ],
    },
    {
        "title": "Reference",
        "url": "reference/index",
    },
],
```

**Limitation:** nav_links URLs are plain strings, not validated at build time. When pages are renamed or moved, nav_links must be updated manually. Mitigation: URLs are generated at setup time from the page inventory, so they are consistent at generation. If pages are later restructured, update nav_links in conf.py and run `sphinx-build` with `-W` to catch broken references in toctrees (though nav_links themselves are not checked). Consider running a linkcheck build (`make linkcheck`) after restructuring.

#### Strategy 2: Single "Docs" Tab

One "Docs" dropdown in the top navbar with all documentation sections as children items.

- A single nav_link entry with title "Docs" and children listing each section index.
- The sidebar shows the full docs tree under the Docs tab.
- The root `index.rst` acts as the "Docs" landing page.

Example conf.py nav_links for single Docs tab:

```python
"nav_links": [
    {
        "title": "Docs",
        "url": "index",
        "children": [
            {
                "title": "Tutorials",
                "url": "tutorials/index",
                "summary": "Step-by-step learning guides"
            },
            {
                "title": "How-To Guides",
                "url": "how-to/index",
                "summary": "Goal-oriented task instructions"
            },
            {
                "title": "Explanation",
                "url": "explanation/index",
                "summary": "Background concepts and design rationale"
            },
            {
                "title": "Reference",
                "url": "reference/index",
                "summary": "API documentation and specifications"
            },
        ],
    },
],
```

#### Strategy 3: Sidebar Only

All navigation in the left sidebar, no top tabs. No nav_links configured.

- No `nav_links` key in `html_theme_options` (or the key is absent entirely).
- The sidebar shows the full toctree from root index.rst.
- Sections are rendered as expandable groups in the sidebar.
- Best for projects with fewer than 6 pages where tabs add overhead.

### Left Nav Depth

- Keep left nav to 3 levels maximum (section heading -> page -> sub-page). Deeper nesting creates narrow, scrolling nav panels that frustrate readers.
- Level 1: section heading (e.g., "Authentication"). Level 2: page (e.g., "OAuth Setup"). Level 3: sub-page as a child entry. Avoid Level 4.
- Use `index.rst` files for section landing pages. Each section landing page should have a toctree listing the section's pages with `:maxdepth: 2`.
- Root `index.rst` should use a hidden toctree (`:hidden:`) listing all Diataxis section indexes so the sidebar populates without rendering a visible list on the landing page.

### Section Index Pages with Abstracts

Section index pages (e.g., `tutorials/index.rst`) serve as landing pages for each documentation section. They list child pages with a 1-sentence abstract beneath each link.

- Each child page should have `.. meta:: :description: One-sentence abstract` at the top. This provides machine-readable metadata for HTML meta tags and social sharing.
- The section index page writes abstracts INLINE beneath each link -- readers see the abstracts directly on the index page, not pulled from metadata automatically.
- Neither Sphinx nor Shibuya auto-renders child page descriptions on the parent index page. The abstracts must be written explicitly.
- Use sphinx-design grids or a simple link list with descriptions for visual presentation.
- Group links by sub-section if the section has 6+ children.

Example section index page (RST):

```rst
.. meta::
   :description: Step-by-step learning guides for new users

Tutorials
=========

Learn to use the library from scratch with these step-by-step guides.

- :ref:`getting-started` -- Install the library and run your first example in under 5 minutes.
- :ref:`first-app` -- Walk through building a complete application from an empty directory to a working deployment.

.. toctree::
   :hidden:

   getting-started
   first-app
```

Example child page metadata:

```rst
.. meta::
   :description: Install the library and run your first example in under 5 minutes

Getting Started
===============

...page content...
```

## Page Metadata

- RST pages do not use YAML frontmatter. Use field lists at the top of a page for metadata, or `:orphan:` to exclude a page from toctree warnings.
- Use page titles (RST heading underlines) as the canonical title. Do not duplicate the title in metadata fields.
- For tutorials, include difficulty context in the opening paragraph rather than metadata fields. RST has no built-in mechanism for structured page metadata like MkDocs frontmatter.

## Cross-References

- Use `:ref:\`label\`` for ALL internal links. Place labels (`.. _label-name:`) above headings. Labels survive page moves and restructuring; file-path references do not.
- STRONGLY DISCOURAGE `:doc:\`path\`` for internal navigation. It couples links to file paths. When pages move, every `:doc:` reference breaks. Use `:ref:` with descriptive labels instead.
- Use intersphinx roles for external Sphinx project links: `:py:class:\`pathlib.Path\``, `:py:func:\`os.path.join\``. These resolve via `intersphinx_mapping` in conf.py, so links stay correct across versions.
- Inline code mentions of project APIs should use domain roles: `:py:func:\`my_func\``, `:py:class:\`MyClass\``, `:py:meth:\`MyClass.method\``. When sphinx-autoapi is enabled, these link to the auto-generated reference pages automatically.
- Example:

  ```rst
  .. _auth-setup:

  Authentication Setup
  ====================

  See :ref:`auth-setup` from any page. For Python's built-in
  :py:class:`pathlib.Path`, the link resolves via intersphinx.
  ```

## sphinx-autoapi Integration

- autoapi output REPLACES the Reference tab entirely. The Author agent MUST NOT write manual reference pages when autoapi is enabled. Docstrings are the source of truth -- manual pages drift out of sync.
- Google docstring style is the convention. All docstrings should follow Google style with Parameters, Returns, Raises, and Example sections.
- Place autoapi output under `reference/api/` using `autoapi_root = "reference/api"` in conf.py.
- Set `autoapi_add_toctree_entry = False` and manually include `api/index` in `reference/index.rst` toctree. This prevents autoapi from injecting at the root toctree level, which would break the clean Diataxis tab structure.
- Set `autoapi_python_class_content = "both"` to show both class-level and `__init__` docstrings. Users need to see both the class purpose and constructor parameters.
- Include `sphinx.ext.napoleon` in conf.py extensions alongside `autoapi.extension`. Napoleon parses Google-style docstrings into structured parameter lists.

## Mermaid Diagrams

Sphinx renders mermaid diagrams via the `sphinxcontrib-mermaid` extension (already included in the scaffold conf.py). The extension must be installed: `pip install sphinxcontrib-mermaid`.

- Use the `.. mermaid::` RST directive. The diagram source is indented under the directive:

  ```rst
  .. mermaid::

     sequenceDiagram
        participant User
        participant API
        User->>API: POST /items
        API-->>User: 201 Created
  ```

- Shibuya theme handles dark/light mode diagram rendering automatically.
- The scaffold sets `mermaid_version = ""` which uses the bundled version from sphinxcontrib-mermaid. Do not override this unless a specific mermaid.js version is required.
- Do NOT use fenced code blocks (` ```mermaid `) in RST files -- Sphinx does not recognize them. Always use the `.. mermaid::` directive.
