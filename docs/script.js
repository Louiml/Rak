(function () {
  'use strict';

  // Mark the document as scripted before anything renders, so the stylesheet
  // knows it is safe to hide the not-yet-revealed elements. Without this the
  // reveal state is applied by CSS alone and a failed script leaves a blank page.
  document.documentElement.classList.add('js');

  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // ============================================================
  // Markdown-driven docs viewer (docs.html)
  // The content lives in content/*.md; docs.html only renders it.
  // ============================================================

  var DOC_PAGES = [
    { group: 'Start', file: 'introduction.md', title: 'Introduction' },
    { group: 'Start', file: 'installation.md', title: 'Installation' },
    { group: 'Start', file: 'quick-start.md', title: 'Quick start' },

    { group: 'Language', file: 'language.md', title: 'Language reference' },
    { group: 'Language', file: 'errors.md', title: 'Error handling' },
    { group: 'Language', file: 'concurrency.md', title: 'Concurrency & async' },
    { group: 'Language', file: 'streams.md', title: 'Streams & data' },
    { group: 'Language', file: 'modules.md', title: 'Modules' },
    { group: 'Language', file: 'macros.md', title: 'Macros' },

    { group: 'Systems', file: 'ffi.md', title: 'FFI & mmap' },
    { group: 'Systems', file: 'networking.md', title: 'Networking & protocols' },
    { group: 'Systems', file: 'vpn.md', title: 'VPN & tunneling' },
    { group: 'Systems', file: 'forensics.md', title: 'Forensics' },
    { group: 'Systems', file: 'safety.md', title: 'Safety' },

    { group: 'Reference', file: 'stdlib.md', title: 'Standard library' },
    { group: 'Reference', file: 'batteries.md', title: 'Stdlib batteries' },
    { group: 'Reference', file: 'osint.md', title: 'OSINT pack' },
    { group: 'Reference', file: 'cli.md', title: 'CLI reference' },
    { group: 'Reference', file: 'tooling.md', title: 'Tooling' },
    { group: 'Reference', file: 'vm.md', title: 'Bytecode VM' },
    { group: 'Reference', file: 'examples.md', title: 'Examples' },
    { group: 'Reference', file: 'project.md', title: 'Project' }
  ];

  var MD_ROOT = 'content/';

  function slugify(s) {
    return s.toLowerCase()
      .replace(/[^a-z0-9\s-]/g, '')
      .trim()
      .replace(/\s+/g, '-');
  }

  // Map a markdown link to the SPA route. `page.html` -> `#page`,
  // `page.html#anchor` -> `#page/anchor`, `#anchor` -> `#<current>/anchor`.
  // External and relative-to-repo links pass through.
  function normalizeDocHref(href, currentPage) {
    if (/^(https?:|mailto:|data:)/i.test(href)) return href;
    var m = href.match(/^([A-Za-z0-9-]+)(\.html)?(#.*)?$/);
    if (m && m[1] !== 'docs' && m[1] !== 'index' && pageByFile(m[1] + '.md')) {
      return '#' + m[1] + '/' + (m[3] ? m[3].slice(1) : '');
    }
    if (href.charAt(0) === '#' && href.length > 1) return '#' + currentPage + '/' + href.slice(1);
    return href;
  }

  function pageByFile(file) {
    for (var i = 0; i < DOC_PAGES.length; i++) if (DOC_PAGES[i].file === file) return DOC_PAGES[i];
    return null;
  }

  function pageTitle(file) {
    var p = pageByFile(file);
    return p ? p.title : file;
  }

  // Inline transforms. `htmlEscaped` text in -> html out. Inline code is
  // lifted into placeholders first so bold/italic/link processing cannot
  // touch its content.
  function mdInline(text, currentPage) {
    var codes = [];
    text = text.replace(/`([^`]+)`/g, function (_, c) {
      codes.push('<code class="md-code">' + c + '</code>');
      return '\u0000' + (codes.length - 1) + '\u0000';
    });
    text = text.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, function (_, label, href) {
      return '<a href="' + escapeHTML(normalizeDocHref(href, currentPage)) + '">' + label + '</a>';
    });
    text = text.replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>');
    text = text.replace(/(^|\W)\*([^*\s][^*]*)\*/g, '$1<i>$2</i>');
    text = text.replace(/\\([\\`*_{}\[\]()#+\-.!|>~])/g, '$1');
    text = text.replace(/\u0000(\d+)\u0000/g, function (_, i) { return codes[+i]; });
    return text;
  }

  // A tiny CommonMark-ish renderer: fenced code, ATX headings, pipe tables,
  // lists (2-space nesting), blockquotes, hr, and paragraphs. This is the
  // only dependency-free renderer used by the docs viewer.
  function renderMarkdown(src, currentPage) {
    var lines = src.replace(/\r\n/g, '\n').split('\n');
    var out = [];
    var i = 0;

    function openCodeblock(lang) {
      lang = (lang || 'text').trim();
      return '<figure class="code-block" data-title="' + escapeHTML(lang) + '">' +
        '<div class="code-block-bar" aria-hidden="true"><span class="bar-dot"></span><span class="bar-dot"></span>' +
        '<span class="bar-dot"></span><span class="bar-title">' + escapeHTML(lang) + '</span></div>' +
        '<pre><code data-lang="' + escapeHTML(lang) + '">';
    }

    while (i < lines.length) {
      var line = lines[i];

      // fenced code
      var fence = line.match(/^```\s*([A-Za-z0-9+#.-]*)\s*$/);
      if (fence) {
        var buf = [];
        i++;
        while (i < lines.length && !/^```/.test(lines[i])) { buf.push(lines[i]); i++; }
        i++; // closing fence
        out.push(openCodeblock(fence[1]) + escapeHTML(buf.join('\n')) + '</code></pre></figure>');
        continue;
      }

      // heading
      var h = line.match(/^(#{1,6})\s+(.*)$/);
      if (h) {
        var level = h[1].length;
        var raw = h[2].replace(/\s+#+\s*$/, '');
        var plain = raw.replace(/`([^`]+)`/g, '$1');
        var id = slugify(plain);
        // The display scale lives in the stylesheet; the renderer only picks
        // which step of it a heading sits at.
        var scale = { 1: 'display-lg', 2: 'display-md', 3: 'display-sm', 4: 'title-md', 5: 'title-sm', 6: 'title-sm' }[level];
        out.push('<h' + level + (level <= 3 ? ' id="' + id + '"' : '') +
          (scale ? ' class="' + scale + '"' : '') + '>' +
          mdInline(escapeHTML(raw), currentPage) + '</h' + level + '>');
        i++;
        continue;
      }

      // table
      if (/^\s*\|/.test(line) && i + 1 < lines.length && /^\s*\|[\s:|-]+\|?\s*$/.test(lines[i + 1])) {
        var rows = [];
        while (i < lines.length && /^\s*\|/.test(lines[i])) { rows.push(lines[i]); i++; }
        var head = splitRow(rows[0]);
        var bodyRows = rows.slice(2);
        var html = '<div class="md-tablewrap"><table class="md-table"><thead><tr>';
        head.forEach(function (c) { html += '<th>' + mdInline(c, currentPage) + '</th>'; });
        html += '</tr></thead><tbody>';
        bodyRows.forEach(function (r) {
          html += '<tr>';
          splitRow(r).forEach(function (c) { html += '<td>' + mdInline(c, currentPage) + '</td>'; });
          html += '</tr>';
        });
        html += '</tbody></table></div>';
        out.push(html);
        continue;
      }

      // hr
      if (/^\s*(---+|\*\*\*+)\s*$/.test(line)) { out.push('<hr>'); i++; continue; }

      // blockquote
      if (/^\s*>/.test(line)) {
        var q = [];
        while (i < lines.length && /^\s*>/.test(lines[i])) { q.push(lines[i].replace(/^\s*>\s?/, '')); i++; }
        out.push('<blockquote>' + renderMarkdown(q.join('\n'), currentPage) + '</blockquote>');
        continue;
      }

      // lists (recursive nesting by indent)
      if (/^(\s*)([-*]|\d+\.)\s+/.test(line)) {
        var baseIndent = line.match(/^(\s*)/)[0].replace(/\t/g, '  ').length;
        var items = [];
        while (i < lines.length) {
          var lm = lines[i].match(/^(\s*)([-*]|\d+\.)\s+(.*)$/);
          if (lm) {
            items.push({
              indent: lm[1].replace(/\t/g, '  ').length - baseIndent,
              ordered: /\d+\./.test(lm[2]),
              text: lm[3]
            });
            i++;
            continue;
          }
          // continuation line: indented plain text continues the last item
          var cont = lines[i].match(/^( +)(\S.*)$/);
          if (cont && cont[1].length >= 2 && items.length &&
              !/^```/.test(lines[i]) && !/^#{1,6}\s/.test(lines[i]) &&
              !/^\s*\|/.test(lines[i]) && !/^\s*>/.test(lines[i])) {
            items[items.length - 1].text += ' ' + cont[2].trim();
            i++;
            continue;
          }
          break;
        }
        var state = { i: 0 };
        out.push(buildList(items, state, 0, currentPage));
        continue;
      }

      // blank
      if (/^\s*$/.test(line)) { i++; continue; }

      // paragraph
      var para = [];
      while (i < lines.length && !/^\s*$/.test(lines[i]) &&
             !/^```/.test(lines[i]) && !/^#{1,6}\s/.test(lines[i]) &&
             !/^\s*\|/.test(lines[i]) && !/^\s*>/.test(lines[i]) &&
             !/^(\s*)([-*]|\d+\.)\s+/.test(lines[i]) &&
             !/^\s*(---+|\*\*\*+)\s*$/.test(lines[i])) {
        para.push(lines[i].trim());
        i++;
      }
      if (para.length) out.push('<p>' + mdInline(escapeHTML(para.join(' ')), currentPage) + '</p>');
    }

    return out.join('\n');
  }

  // Build a nested list HTML from flat items. Nested items (larger indent)
  // are spliced inside the preceding <li>.
  function buildList(items, state, indent, currentPage) {
    var tag = items[state.i].ordered ? 'ol' : 'ul';
    var html = '<' + tag + '>';
    while (state.i < items.length) {
      var it = items[state.i];
      if (it.indent < indent) break;
      if (it.indent > indent) {
        var nested = buildList(items, state, it.indent, currentPage);
        html = html.replace(/<\/li>$/, nested + '</li>');
        continue;
      }
      html += '<li>' + mdInline(escapeHTML(it.text), currentPage) + '</li>';
      state.i++;
    }
    html += '</' + tag + '>';
    return html;
  }

  function splitRow(row) {
    var s = row.trim();
    if (s.charAt(0) === '|') s = s.slice(1);
    if (s.charAt(s.length - 1) === '|') s = s.slice(0, -1);
    s = s.replace(/\\\|/g, '\u0001');
    var cells = s.split('|').map(function (c) {
      return c.replace(/\u0001/g, '|').trim();
    });
    return cells;
  }

  // ------------------------------------------------------------
  // Viewer: sidebar, router, fetch + render
  // ------------------------------------------------------------

  function initDocsViewer() {
    var tocEl = document.getElementById('docs-pages');
    var contentEl = document.getElementById('md-content');
    if (!tocEl || !contentEl) return;

    var currentFile = null;

    function buildSidebar(filter) {
      var q = (filter || '').trim().toLowerCase();
      var groups = [];
      DOC_PAGES.forEach(function (p) {
        if (q && p.title.toLowerCase().indexOf(q) === -1) return;
        if (!groups.length || groups[groups.length - 1].name !== p.group) {
          groups.push({ name: p.group, pages: [] });
        }
        groups[groups.length - 1].pages.push(p);
      });

      var html = '<div class="toc-filter"><input type="text" id="toc-filter-input" placeholder="Filter pages&hellip;" aria-label="Filter pages"></div>';
      groups.forEach(function (g) {
        html += '<span class="toc-group">' + escapeHTML(g.name) + '</span>';
        g.pages.forEach(function (p) {
          var slug = p.file.replace(/\.md$/, '');
          var cls = p.file === currentFile ? ' class="active"' : '';
          html += '<a href="#' + slug + '"' + cls + ' data-file="' + p.file + '">' + escapeHTML(p.title) + '</a>';
        });
      });
      // html += '<div class="toc-meta"><b>' + DOC_PAGES.length + ' pages &middot;</b> rendered from <code>content/*.md</code></div>';
      tocEl.innerHTML = html;

      var input = document.getElementById('toc-filter-input');
      if (input) {
        input.value = filter || '';
        input.addEventListener('input', function () { buildSidebar(input.value); });
        input.focus();
      }
    }

    function buildPageToc(md) {
      var html = '<span class="toc-group">On this page</span>';
      var found = false;
      md.replace(/\r\n/g, '\n').split('\n').forEach(function (l) {
        var m = l.match(/^##\s+(.*)$/);
        if (!m) return;
        var raw = m[1].replace(/\s+#+\s*$/, '').replace(/`([^`]+)`/g, '$1');
        found = true;
        html += '<a href="#' + currentFile.replace(/\.md$/, '') + '/' + slugify(raw) + '">' + escapeHTML(raw) + '</a>';
      });
      var container = document.getElementById('toc-page');
      if (!container) {
        container = document.createElement('div');
        container.id = 'toc-page';
        container.className = 'toc-page';
        tocEl.appendChild(container);
      }
      container.innerHTML = found ? html : '';
    }

    function setActiveLink() {
      var slug = currentFile && currentFile.replace(/\.md$/, '');
      tocEl.querySelectorAll('a[data-file]').forEach(function (a) {
        a.classList.toggle('active', a.getAttribute('data-file') === currentFile);
      });
    }

    function scrollIntoPage(hash) {
      var idx = hash.indexOf('/');
      if (idx === -1) { window.scrollTo(0, 0); return; }
      var id = hash.slice(idx + 1);
      var el = document.getElementById(id);
      if (el) {
        var y = el.getBoundingClientRect().top + window.scrollY - 84;
        if (reduceMotion) window.scrollTo(0, y);
        else window.scrollTo({ top: y, behavior: 'smooth' });
      }
    }

    function renderFile(file, hash) {
      var slug = file.replace(/\.md$/, '');
      contentEl.innerHTML = '<div class="md-loading">Loading &ldquo;' + escapeHTML(pageTitle(file)) + '&rdquo;&hellip;</div>';
      fetch(MD_ROOT + file, { cache: 'no-cache' })
        .then(function (resp) {
          if (!resp.ok) throw new Error('HTTP ' + resp.status);
          return resp.text();
        })
        .then(function (text) {
          currentFile = file;
          var html = renderMarkdown(text, slug);
          var pi = DOC_PAGES.findIndex(function (p) { return p.file === file; });
          var pager = '';
          if (pi > 0) pager += '<a class="md-pager prev" href="#' + DOC_PAGES[pi - 1].file.replace(/\.md$/, '') + '">&larr; ' + escapeHTML(DOC_PAGES[pi - 1].title) + '</a>';
          if (pi < DOC_PAGES.length - 1) pager += '<a class="md-pager next" href="#' + DOC_PAGES[pi + 1].file.replace(/\.md$/, '') + '">' + escapeHTML(DOC_PAGES[pi + 1].title) + ' &rarr;</a>';
          contentEl.innerHTML = '<section class="doc-section">' + html + '</section>' +
            (pager ? '<nav class="md-pager-row">' + pager + '</nav>' : '');
          document.title = 'Rak Docs: ' + pageTitle(file);
          buildSidebarKeepFilter();
          buildPageToc(text);
          setActiveLink();
          highlightBlocks();
          initCopyButtons();
          scrollIntoPage(hash);
        })
        .catch(function () {
          contentEl.innerHTML =
            '<div class="md-error">' +
            '<h2>Could not load <code>content/' + escapeHTML(file) + '</code></h2>' +
            '<p>The docs viewer fetches its markdown pages at runtime. Browsers block <code>fetch()</code> from <code>file://</code> URLs &mdash; serve the <code>docs/</code> folder over HTTP, e.g.:</p>' +
            '<figure class="code-block" data-title="shell"><div class="code-block-bar" aria-hidden="true"><span class="bar-dot"></span><span class="bar-dot"></span><span class="bar-dot"></span><span class="bar-title">shell</span></div>' +
            '<pre><code data-lang="text">cd docs &amp;&amp; python -m http.server 8000\n# then open http://localhost:8000/docs.html</code></pre></figure>' +
            '<p>On GitHub Pages this resolves automatically.</p>' +
            '</div>';
        });
    }

    // Rebuild the sidebar preserving any active filter text.
    function buildSidebarKeepFilter() {
      var input = document.getElementById('toc-filter-input');
      buildSidebar(input ? input.value : '');
    }

    function route() {
      var hash = window.location.hash.replace(/^#/, '');
      var slug = hash.indexOf('/') === -1 ? hash : hash.slice(0, hash.indexOf('/'));
      var file = slug ? slug + '.md' : DOC_PAGES[0].file;
      if (!pageByFile(file)) file = DOC_PAGES[0].file;
      if (file === currentFile) {
        scrollIntoPage(hash);
        return;
      }
      renderFile(file, hash);
    }

    window.addEventListener('hashchange', route);
    buildSidebar('');
    route();
  }

  // ============================================================
  // Rak syntax highlighting (shared by index.html and docs.html)
  // ============================================================

  var KEYWORDS = new Set([
    'let', 'mut', 'fn', 'return', 'if', 'else', 'for', 'while', 'loop', 'break', 'continue',
    'use', 'dump', 'trace', 'scan', 'fetch', 'in', 'struct', 'enum', 'impl', 'trait', 'match',
    'try', 'catch', 'raise', 'throw', 'async', 'await', 'spawn', 'type', 'as', 'mod', 'pub',
    'macro', 'const', 'extern', 'import', 'from', 'export', 'pipe',
    'defer', 'test', 'assert'
  ]);
  // Keywords that produce a value (so a following '/' is division, not a regex).
  var VALUE_WORDS = new Set(['true', 'false', 'nil', 'open', 'port', 'banner']);
  var TYPES = new Set([
    'int', 'string', 'char', 'bytes', 'bool', 'hex', 'hex8', 'hex16', 'hex32', 'hex64',
    'i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64', 'f32', 'f64',
    'Option', 'Result', 'Display', 'Debug', 'Iterable', 'Index', 'IndexMut',
    'Some', 'None', 'Ok', 'Err'
  ]);
  var BUILTINS = new Set([
    'fmt', 'len', 'split', 'join', 'contains', 'upper', 'lower', 'trim', 'replace', 'find',
    'starts_with', 'ends_with', 'slice', 'repeat', 'reverse', 'sum', 'min', 'max', 'abs',
    'sqrt', 'pow', 'clamp', 'sort', 'push', 'keys', 'values', 'has', 'get', 'read', 'write',
    'array', 'map', 'print', 'dbg', 'exit', 'sleep', 'now_ms', 'args', 'env_get', 'ord',
    'chr', 'substr', 'md5', 'sha1', 'sha256', 'xor', 'rot13', 'hex_encode', 'hex_decode',
    'base64_encode', 'base64_decode', 'url_encode', 'url_decode', 'to_hex', 'from_hex',
    'dns_lookup', 'reverse_dns', 'scan_ports', 'scan_subdomains', 'net_listen', 'net_accept',
    'net_connect', 'net_local_addr', 'tcp_read', 'tcp_write', 'tcp_read_line', 'tcp_close',
    'thread_join', 'channel', 'chan_send', 'chan_recv', 'regex_new', 'regex_match',
    'regex_is_match', 'regex_find', 'regex_find_all', 'regex_replace', 'html_title',
    'html_select', 'html_links', 'html_images', 'html_forms', 'html_count', 'file_write',
    'file_read', 'file_exists', 'file_size', 'file_list', 'file_delete', 'file_mkdir',
    'file_copy', 'json_parse', 'json_stringify', 'json_get', 'json_path', 'json_keys',
    'json_find_all',
    // systems & OSINT builtins
    'ffi_load', 'ffi_ptr', 'ffi_alloc', 'ffi_read', 'ffi_write', 'ffi_free',
    'ffi_cstr_to_string', 'ffi_string_to_cstr',
    'mmap_open', 'mmap_slice', 'mmap_size', 'mmap_close', 'mmap_find',
    'mmap_lines', 'mmap_lines_off',
    'http_get_async', 'tcp_probe', 'tcp_connect_async',
    'net_raw_ipv4', 'net_raw_tcp', 'net_raw_udp', 'net_raw_tcp_syn', 'net_raw_csum',
    'net_raw_send', 'net_raw_recv',
    'dns_build', 'dns_query', 'dns_parse',
    'tls_parse_client_hello', 'tls_parse_cert_chain',
    'pcap_open', 'pcap_next'
  ]);
  var TWO = ['|>', '=>', '::', '..', '==', '!=', '<=', '>=', '&&', '||', '<<', '>>', '->', '+=', '-=', '*=', '/=', '%='];

  function escapeHTML(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function readString(line, i) {
    // i points at the opening quote
    var j = i + 1;
    while (j < line.length) {
      if (line[j] === '\\' && j + 1 < line.length) { j += 2; continue; }
      if (line[j] === '"') { j++; break; }
      j++;
    }
    return { text: line.slice(i, j), next: j };
  }

  function readChar(line, i) {
    // i points at the opening '
    var j = i + 1;
    var rest = line.slice(j);
    var uni = rest.match(/^\\u\{[0-9A-Fa-f]{1,6}\}/);
    if (uni) {
      j += uni[0].length;
    } else if (line[j] === '\\' && line[j + 1] === 'x' && j + 3 < line.length && /[0-9a-fA-F]{2}/.test(line.slice(j + 2, j + 4))) {
      j += 4;
    } else if (line[j] === '\\' && j + 1 < line.length) {
      j += 2;
    } else if (line[j] && line[j] !== "'") {
      j += 1;
    }
    if (line[j] === "'") j++;
    return { text: line.slice(i, j), next: j };
  }

  function readRegex(line, i) {
    // i points at the opening /
    var j = i + 1;
    var inClass = false;
    while (j < line.length) {
      var c = line[j];
      if (c === '\\' && j + 1 < line.length) { j += 2; continue; }
      if (c === '[') { inClass = true; j++; continue; }
      if (c === ']') { inClass = false; j++; continue; }
      if (c === '/' && !inClass) { j++; break; }
      j++;
    }
    if (j > line.length || line[j - 1] !== '/') return null; // unterminated
    while (j < line.length && /[a-z]/i.test(line[j])) j++;
    return { text: line.slice(i, j), next: j };
  }

  function highlightLine(line) {
    var out = '';
    var i = 0;
    var prevCanEnd = false; // can the previous significant token end an expression?

    function emit(cls, text) {
      if (cls) out += '<span class="' + cls + '">' + escapeHTML(text) + '</span>';
      else out += escapeHTML(text);
    }
    function sig(cls, text) {
      if (!cls) return; // whitespace doesn't change context
      if (cls === 'tok-op') {
        prevCanEnd = (text === ')' || text === ']' || text === '}');
      } else if (cls === 'tok-keyword') {
        prevCanEnd = VALUE_WORDS.has(text);
      } else {
        prevCanEnd = true; // number, string, char, regex, type, builtin, ident
      }
    }

    while (i < line.length) {
      var c = line[i];

      // comment
      if (c === '/' && line[i + 1] === '/') { emit('tok-comment', line.slice(i)); break; }

      // string (and f"/b" prefixes)
      if (c === '"') { var s = readString(line, i); emit('tok-string', s.text); i = s.next; sig('tok-string', s.text); continue; }
      if ((c === 'f' || c === 'b') && line[i + 1] === '"') { var s2 = readString(line, i + 1); emit('tok-string', c + s2.text); i = s2.next; sig('tok-string', s2.text); continue; }

      // char literal
      if (c === "'") { var ch = readChar(line, i); emit('tok-char', ch.text); i = ch.next; sig('tok-char', ch.text); continue; }

      // regex literal (only in operand context)
      if (c === '/' && !prevCanEnd) { var r = readRegex(line, i); if (r) { emit('tok-regex', r.text); i = r.next; sig('tok-regex', r.text); continue; } }

      // hex
      if (c === '0' && (line[i + 1] === 'x' || line[i + 1] === 'X')) {
        var j = i + 2; while (j < line.length && /[0-9A-Fa-f]/.test(line[j])) j++;
        var hx = line.slice(i, j); emit('tok-number', hx); i = j; sig('tok-number', hx); continue;
      }

      // binary and octal base literals: 0b1010, 0o755
      if (c === '0' && (line[i + 1] === 'b' || line[i + 1] === 'B')) {
        var j = i + 2; while (j < line.length && /[01_]/.test(line[j])) j++;
        var bx = line.slice(i, j); emit('tok-number', bx); i = j; sig('tok-number', bx); continue;
      }
      if (c === '0' && (line[i + 1] === 'o' || line[i + 1] === 'O')) {
        var j = i + 2; while (j < line.length && /[0-7_]/.test(line[j])) j++;
        var ox = line.slice(i, j); emit('tok-number', ox); i = j; sig('tok-number', ox); continue;
      }

      // number (with type suffix)
      if (/[0-9]/.test(c)) {
        var j = i; while (j < line.length && /[0-9.]/.test(line[j])) j++;
        var suf = line.slice(j).match(/^(f32|f64|i8|i16|i32|i64|u8|u16|u32|u64)/);
        if (suf) j += suf[0].length;
        var num = line.slice(i, j); emit('tok-number', num); i = j; sig('tok-number', num); continue;
      }

      // macro var placeholder ($param)
      if (c === '$' && /[a-zA-Z_]/.test(line[i + 1] || '')) {
        var j = i + 1; while (j < line.length && /[a-zA-Z0-9_]/.test(line[j])) j++;
        var mv = line.slice(i, j); emit('tok-keyword', mv); i = j; continue;
      }

      // identifier / keyword / type / builtin
      if (/[a-zA-Z_]/.test(c)) {
        var j = i; while (j < line.length && /[a-zA-Z0-9_]/.test(line[j])) j++;
        var word = line.slice(i, j);
        var cls = null;
        if (KEYWORDS.has(word)) cls = 'tok-keyword';
        else if (TYPES.has(word)) cls = 'tok-type';
        else if (BUILTINS.has(word)) cls = 'tok-builtin';
        emit(cls, word); i = j; sig(cls, word); continue;
      }

      // two-char operators
      var two = line.slice(i, i + 2);
      if (TWO.indexOf(two) !== -1) { emit('tok-op', two); i += 2; sig('tok-op', two); continue; }

      // single-char operators
      if ('+-*/%&|^!~<>=.,:;()[]{}?'.indexOf(c) !== -1) { emit('tok-op', c); i++; sig('tok-op', c); continue; }

      // whitespace
      if (/\s/.test(c)) { var w = line.slice(i).match(/^\s+/)[0]; emit(null, w); i += w.length; continue; }

      // anything else
      emit(null, c); i++;
    }
    return out;
  }

  function highlightBlocks() {
    document.querySelectorAll('code[data-lang="rak"]').forEach(function (code) {
      if (code.dataset.highlighted) return;
      var lines = code.textContent.split('\n');
      code.innerHTML = lines.map(highlightLine).join('\n');
      code.dataset.highlighted = '1';
    });
  }

  function initCopyButtons() {
    document.querySelectorAll('.code-block').forEach(function (block) {
      var pre = block.querySelector('pre');
      if (!pre) return;
      if (block.querySelector('.copy-btn')) return;

      var btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'copy-btn';
      btn.textContent = 'Copy';
      btn.setAttribute('aria-label', 'Copy code');
      block.appendChild(btn);

      btn.addEventListener('pointerdown', function (e) { e.currentTarget.classList.add('pressed'); });
      btn.addEventListener('click', function () {
        var text = pre.textContent;
        var done = function () {
          btn.textContent = 'Copied';
          btn.classList.add('copied');
          window.setTimeout(function () { btn.textContent = 'Copy'; btn.classList.remove('copied'); }, 1800);
        };
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(text).then(done, function () { fallbackCopy(text); done(); });
        } else { fallbackCopy(text); done(); }
      });
      btn.addEventListener('pointerup', function () { btn.classList.remove('pressed'); });

      function fallbackCopy(text) {
        var ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand('copy'); } catch (err) {}
        document.body.removeChild(ta);
      }
    });
  }

  function initReveals() {
    var els = document.querySelectorAll('.reveal');
    if (reduceMotion || !('IntersectionObserver' in window)) {
      els.forEach(function (el) { el.classList.add('is-visible'); });
      return;
    }
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) { entry.target.classList.add('is-visible'); io.unobserve(entry.target); }
      });
    }, { threshold: 0.12, rootMargin: '0px 0px -8% 0px' });
    els.forEach(function (el) { io.observe(el); });
  }

  function initProgressBars() {
    var bars = document.querySelectorAll('.progress span[data-width]');
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.style.width = entry.target.getAttribute('data-width') + '%';
          io.unobserve(entry.target);
        }
      });
    }, { threshold: 0.4 });
    bars.forEach(function (bar) {
      if (reduceMotion) bar.style.width = bar.getAttribute('data-width') + '%';
      else io.observe(bar);
    });
  }

  function initScrollSpy() {
    var sections = Array.prototype.slice.call(document.querySelectorAll('section.doc-section[id]'));
    var links = Array.prototype.slice.call(document.querySelectorAll('.docs-toc a[href^="#"]'));
    var navLinks = Array.prototype.slice.call(document.querySelectorAll('.site-nav a[href^="#"]'));
    var allLinks = links.concat(navLinks);
    if (!sections.length || !allLinks.length) return;

    var activeEls = { toc: links[0], nav: navLinks[0] };
    function setActive(id) {
      if (activeEls.toc) {
        activeEls.toc.classList.remove('active');
        activeEls.toc = links.filter(function (l) { return l.getAttribute('href') === '#' + id; })[0] || null;
        if (activeEls.toc) activeEls.toc.classList.add('active');
      }
      if (activeEls.nav) {
        activeEls.nav.classList.remove('active');
        activeEls.nav = navLinks.filter(function (l) { return l.getAttribute('href') === '#' + id; })[0] || null;
        if (activeEls.nav) activeEls.nav.classList.add('active');
      }
    }

    var ticking = false;
    window.addEventListener('scroll', function () {
      if (ticking) return;
      ticking = true;
      window.requestAnimationFrame(function () {
        var pos = window.scrollY;
        var current = sections[0].getAttribute('id');
        sections.forEach(function (sec) { if (pos >= sec.offsetTop - 140) current = sec.getAttribute('id'); });
        setActive(current);
        ticking = false;
      });
    }, { passive: true });

    allLinks.forEach(function (link) {
      link.addEventListener('click', function (e) {
        if (reduceMotion) return;
        var id = link.getAttribute('href');
        if (!id || id.charAt(0) !== '#') return;
        var target = document.querySelector(id);
        if (!target) return;
        e.preventDefault();
        var y = target.getBoundingClientRect().top + window.scrollY - 84;
        window.scrollTo({ top: y, behavior: 'smooth' });
        history.replaceState(null, '', id);
      });
    });
  }

  function initThemeToggle() {
    var root = document.documentElement;
    var btn = document.getElementById('theme-toggle');
    if (!btn) return;

    var stored = null;
    try { stored = localStorage.getItem('rak-theme'); } catch (err) {}
    // The brand is the light palette: a pure white canvas with the sky-blue
    // wash behind the hero. Dark is an opt-in inversion of the same tokens,
    // not something the operating system decides - a reader whose machine
    // happens to be in dark mode still gets the page as designed.
    var theme = stored || 'light';

    function apply(t) {
      root.setAttribute('data-theme', t);
      btn.textContent = t === 'dark' ? 'Light' : 'Dark';
      btn.setAttribute('aria-label', t === 'dark' ? 'Switch to the light theme' : 'Switch to the dark theme');
      var meta = document.querySelector('meta[name="theme-color"]');
      if (meta) meta.setAttribute('content', t === 'dark' ? '#171717' : '#ffffff');
      try { localStorage.setItem('rak-theme', t); } catch (err) {}
    }

    apply(theme);

    btn.addEventListener('click', function () {
      apply(root.getAttribute('data-theme') === 'dark' ? 'light' : 'dark');
    });
  }

// ============================================================
// Playground
//
// The site's "try Rak" page. The interpreter is a WebAssembly module
// built from Rak's own source, loaded on first use, and driven through
// a small C ABI: write the program into the module's memory, call
// `run`, read the output back.
//
// The editor is a textarea with a highlighted layer behind it, painted
// by the same highlighter the docs use, so Rak code is coloured while
// you type and the caret still sits where you expect it.
// ============================================================

var PG_EXAMPLES = [
  {
    name: 'Hello',
    file: 'hello.rak',
    code: '// Rak is a language for hackers and OSINT investigators.\n' +
          '// Press Run, or edit and press Run again.\n\n' +
          'dump "Hello, World"\n'
  },
  {
    name: 'Hex and binary patterns',
    file: 'sniff.rak',
    code: '// Match a byte slice by its magic number.\n' +
          'fn sniff(d: bytes) {\n' +
          '    match d {\n' +
          '        [0x89, \'P\', \'N\', \'G\', ..] => { return "png" },\n' +
          '        [0xFF, 0xD8, 0xFF, ..] => { return "jpeg" },\n' +
          '        _ => { return "unknown" },\n' +
          '    }\n' +
          '}\n\n' +
          'dump sniff(b"\\x89PNG\\x0d\\x0a")\n'
  },
  {
    name: 'Pipelines and regex',
    file: 'pipeline.rak',
    code: '// A pipeline reads left to right, like a sentence.\n' +
          'fn inc(n) { return n + 1 }\n' +
          'fn dbl(n) { return n * 2 }\n\n' +
          'dump 5 |> inc |> dbl          // 12\n\n' +
          '// Regex literals are written inline.\n' +
          'let re = /\\d+/g\n' +
          'dump re.find_all("a1 b22 c333")   // [1, 22, 333]\n'
  },
  {
    name: 'Structs and traits',
    file: 'traits.rak',
    code: 'struct Point { x: int, y: int }\n\n' +
          '// A trait implementation drives `dump`.\n' +
          'impl Display for Point {\n' +
          '    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }\n' +
          '}\n\n' +
          'let p = Point { x: 3, y: 4 }\n' +
          'dump p              // (3, 4)\n' +
          'dump p.x            // 3\n'
  },
  {
    name: 'Errors',
    file: 'errors.rak',
    code: '// `try` catches what `raise` throws.\n' +
          'fn risky(n) {\n' +
          '    if n > 10 { raise "too big" }\n' +
          '    return n\n' +
          '}\n\n' +
          'try {\n' +
          '    dump risky(3)\n' +
          '    dump risky(99)\n' +
          '} catch e {\n' +
          '    dump "caught: " + e\n' +
          '}\n'
  },
  {
    name: 'Evidence provenance',
    file: 'evidence.rak',
    code: '// Every value can carry where it came from.\n' +
          'let target = evidence<string> from "93.184.216.34"\n' +
          'let finding = cite("TLS certificate issued", "tls_parse", "example.com")\n\n' +
          'dump report(target, finding)\n'
  },
  {
    name: 'Threads and channels',
    file: 'threads.rak',
    code: '// `spawn` runs a function on its own thread.\n' +
          'let (tx, rx) = channel()\n\n' +
          'let h = spawn(fn() {\n' +
          '    chan_send(tx, "from another thread")\n' +
          '    return 1\n' +
          '})\n\n' +
          'thread_join(h)\n' +
          'dump chan_recv(rx)\n'
  }
];

// The WebAssembly module. The playground page loads it as soon as it opens, so
// the cost is paid by the page that uses it and nothing else.
var pgModule = null;
var pgLoading = null;

  function pgLoad() {
    if (pgModule) return Promise.resolve(pgModule);
    if (pgLoading) return pgLoading;
    pgLoading = fetch('rak.wasm')
      .then(function (r) {
        if (!r.ok) throw new Error('HTTP ' + r.status);
        return r.arrayBuffer();
      })
      .then(function (buf) {
        // The module imports one function: the browser's random bytes. `getrandom`
        // has no wasm backend of its own that does not drag in wasm-bindgen, so
        // the module asks the page for entropy instead.
        var memory = null;
        var imports = {
          rak: {
            random_bytes: function (ptr, len) {
              try {
                var view = new Uint8Array(memory.buffer, ptr, len);
                (window.crypto || window.msCrypto).getRandomValues(view);
                return 0;
              } catch (e) {
                return 1;
              }
            }
          }
        };
        return WebAssembly.instantiate(buf, imports).then(function (inst) {
          memory = inst.instance.exports.memory;
          pgModule = inst.instance.exports;
          return pgModule;
        });
      })
      .catch(function (e) {
        pgLoading = null;
        throw e;
      });
    return pgLoading;
  }

function initPlayground() {
  var codeEl = document.getElementById('pg-code');
  if (!codeEl) return;

  var highlightEl = document.getElementById('pg-highlight');
  var gutterEl = document.getElementById('pg-gutter');
  var outputEl = document.getElementById('pg-output');
  var runBtn = document.getElementById('pg-run');
  var statusEl = document.getElementById('pg-status');
  var selectEl = document.getElementById('pg-example');
  var fileEl = document.getElementById('pg-filename');
  var resetBtn = document.getElementById('pg-reset');
  var clearBtn = document.getElementById('pg-clear');

  // The example picker.
  PG_EXAMPLES.forEach(function (ex, i) {
    var opt = document.createElement('option');
    opt.value = String(i);
    opt.textContent = ex.name;
    selectEl.appendChild(opt);
  });

  function loadExample(i) {
    var ex = PG_EXAMPLES[i];
    if (!ex) return;
    codeEl.value = ex.code;
    fileEl.textContent = ex.file;
    paint();
  }

  // Repaint the highlighted layer and the line-number gutter. The layer sits
  // behind a transparent textarea, so this is the only colouring there is.
  function paint() {
    var text = codeEl.value;
    highlightEl.innerHTML = highlightLine(text) + '\n';
    var lines = text.split('\n').length;
    var nums = [];
    for (var i = 1; i <= lines; i++) nums.push(i);
    gutterEl.textContent = nums.join('\n');
  }

  function setStatus(text, cls) {
    statusEl.textContent = text;
    statusEl.className = 'badge-pill' + (cls ? ' ' + cls : '');
  }

  function escapeHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  function show(text, isError) {
    var lines = text.split('\n');
    var html = '';
    for (var i = 0; i < lines.length; i++) {
      var line = escapeHtml(lines[i]);
      // The error line is highlighted, so a failure is visible in the output
      // without having to read it.
      if (isError && lines[i].indexOf('error:') === 0) line = '<span class="pg-err">' + line + '</span>';
      html += line + (i < lines.length - 1 ? '\n' : '');
    }
    outputEl.innerHTML = html;
    outputEl.className = 'playground-output';
  }

  function run() {
    if (!pgModule) {
      setStatus('loading the interpreter', '');
      outputEl.textContent = 'Loading the interpreter...';
    }
    var source = codeEl.value;
    pgLoad()
      .then(function (exports) {
        var bytes = new TextEncoder().encode(source);
        // Copy the program into the module's memory, at the address the module
        // says its program buffer lives at.
        var mem = new Uint8Array(exports.memory.buffer, exports.program_ptr() >>> 0, bytes.length);
        mem.set(bytes);
        var outLen = exports.run(0, bytes.length) >>> 0;
        // The memory may have grown, which detaches the old view, so it is read
        // again after the call.
        var out = new Uint8Array(exports.memory.buffer, exports.out_ptr() >>> 0, outLen);
        var text = new TextDecoder().decode(out);
        var failed = text.indexOf('error:') !== -1;
        show(text, failed);
        setStatus(failed ? 'error' : 'ok', failed ? 'badge-pill-warning' : '');
      })
      .catch(function (e) {
        show('could not start the interpreter: ' + e, true);
        setStatus('error', 'badge-pill-warning');
      });
  }

  codeEl.addEventListener('input', paint);
  codeEl.addEventListener('keydown', function (e) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      run();
      return;
    }
    // Tab indents rather than leaving the field, which is what a code editor
    // is expected to do.
    if (e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault();
      var s = codeEl.selectionStart;
      var en = codeEl.selectionEnd;
      codeEl.value = codeEl.value.slice(0, s) + '  ' + codeEl.value.slice(en);
      codeEl.selectionStart = codeEl.selectionEnd = s + 2;
      paint();
    }
  });
  runBtn.addEventListener('click', run);
  resetBtn.addEventListener('click', function () {
    loadExample(Number(selectEl.value) || 0);
  });
  clearBtn.addEventListener('click', function () {
    outputEl.textContent = '';
  });
  selectEl.addEventListener('change', function () {
    loadExample(Number(selectEl.value) || 0);
  });

  loadExample(0);
  // The interpreter is fetched as soon as the playground opens, so pressing
  // Run starts the program straight away. The button stays disabled until the
  // module is there, rather than pretending to be ready.
  setStatus('loading the interpreter', '');
  pgLoad().then(
    function () {
      runBtn.disabled = false;
      setStatus('ready', '');
    },
    function () {
      setStatus('could not load the interpreter', 'badge-pill-warning');
    }
  );
}

  document.addEventListener('DOMContentLoaded', function () {
    initDocsViewer();
    highlightBlocks();
    initCopyButtons();
    initReveals();
    initProgressBars();
    initScrollSpy();
    initThemeToggle();
    initPlayground();
  });
})();
