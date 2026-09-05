'use client';

import React, { useRef, useEffect, useState, useCallback } from 'react';
import EditorContextMenu, { ContextMenuItem } from './EditorContextMenu';

interface CodeEditorProps {
  value: string;
  onChange: (value: string) => void;
  onRun?: () => void;
}

const KEYWORDS = [
  'scan', 'fetch', 'dump', 'trace', 'loop', 'if', 'else', 'fn', 'let', 'mut',
  'return', 'use', 'mod', 'pub', 'struct', 'enum', 'impl', 'match', 'for',
  'while', 'break', 'continue', 'true', 'false', 'nil', 'in',
  'try', 'catch', 'raise', 'throw', 'trait', 'async', 'await', 'spawn', 'as', 'type',
];

const KEYWORD_SET = new Set(KEYWORDS);

const TYPES = [
  'hex8', 'hex16', 'hex32', 'hex64', 'int', 'string', 'bytes', 'bool',
  'i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64', 'f32', 'f64',
  'Option', 'Result',
];
const TYPE_SET = new Set(TYPES);

const BUILTINS = [
  'fmt', 'md5', 'sha1', 'sha256', 'xor', 'rot13', 'hex_encode', 'hex_decode',
  'base64_encode', 'base64_decode', 'url_encode', 'url_decode', 'dns_lookup',
  'subdomain_enum', 'reverse_dns', 'len', 'split', 'join', 'contains',
  'to_hex', 'from_hex', 'int', 'string', 'bytes', 'float', 'upper', 'lower', 'trim',
  'push', 'read', 'write', 'array', 'map', 'keys', 'values', 'has', 'get', 'sort',
  'file_read', 'file_write', 'file_append', 'file_exists', 'file_size',
  'file_list', 'file_delete', 'file_mkdir', 'file_copy', 'file_rename',
  'file_ext', 'file_basename', 'file_dirname',
  'html_title', 'html_select', 'html_select_all', 'html_attr', 'html_links',
  'html_images', 'html_scripts', 'html_forms', 'html_inputs', 'html_meta',
  'html_count', 'html_headers',
  'json_parse', 'json_get', 'json_path', 'json_keys', 'json_len', 'json_find_all',
  'scan_ports', 'scan_subdomains',
  'net_listen', 'net_accept', 'net_connect', 'net_local_addr',
  'tcp_read', 'tcp_write', 'tcp_read_line', 'tcp_close',
  'spawn', 'thread_join', 'channel', 'chan_send', 'chan_recv',
  'sleep', 'now_ms', 'args', 'env_get', 'ord', 'chr', 'substr', 'print', 'dbg', 'exit',
  'Some', 'None', 'Ok', 'Err',
];

const ALL_SUGGESTIONS = [...KEYWORDS, ...TYPES, ...BUILTINS];

const BRACKETS: Record<string, string> = {
  '(': ')', '[': ']', '{': '}',
};
const CLOSING_BRACKETS = new Set([')', ']', '}']);

interface Token {
  type: 'keyword' | 'type' | 'hex' | 'number' | 'float' | 'typedint' | 'interp' | 'string' | 'comment' | 'bytes' | 'ident' | 'op' | 'ws';
  value: string;
}

function tokenizeLine(line: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;

  while (i < line.length) {
    if (/\s/.test(line[i])) {
      let ws = '';
      while (i < line.length && /\s/.test(line[i])) { ws += line[i]; i++; }
      tokens.push({ type: 'ws', value: ws });
      continue;
    }
    if (line[i] === '/' && line[i + 1] === '/') {
      tokens.push({ type: 'comment', value: line.slice(i) });
      break;
    }
    if (line[i] === '0' && (line[i + 1] === 'x' || line[i + 1] === 'X')) {
      let hex = '0x';
      i += 2;
      while (i < line.length && /[0-9A-Fa-f]/.test(line[i])) { hex += line[i]; i++; }
      tokens.push({ type: 'hex', value: hex });
      continue;
    }
    if (line[i] === 'f' && line[i + 1] === '"') {
      let str = 'f"';
      i += 2;
      while (i < line.length && line[i] !== '"') {
        if (line[i] === '\\' && i + 1 < line.length) { str += line[i] + line[i + 1]; i += 2; }
        else { str += line[i]; i++; }
      }
      if (i < line.length) { str += '"'; i++; }
      tokens.push({ type: 'interp', value: str });
      continue;
    }
    if (/[0-9]/.test(line[i])) {
      let num = '';
      while (i < line.length && /[0-9]/.test(line[i])) { num += line[i]; i++; }
      if (line[i] === '.' && /[0-9]/.test(line[i + 1] || '')) {
        num += '.';
        i++;
        while (i < line.length && /[0-9]/.test(line[i])) { num += line[i]; i++; }
        if (line[i] === 'f' && (line[i + 1] === '3' || line[i + 1] === '6')) { num += 'f' + line[i + 1]; i += 2; }
        tokens.push({ type: 'float', value: num });
        continue;
      }
      const suf = line.slice(i).match(/^(i8|i16|i32|i64|u8|u16|u32|u64|f32|f64)/);
      if (suf) { num += suf[0]; i += suf[0].length; tokens.push({ type: 'typedint', value: num }); continue; }
      tokens.push({ type: 'number', value: num });
      continue;
    }
    if (line[i] === 'b' && line[i + 1] === '"') {
      let str = 'b"';
      i += 2;
      while (i < line.length && line[i] !== '"') {
        if (line[i] === '\\' && i + 1 < line.length) { str += line[i] + line[i + 1]; i += 2; }
        else { str += line[i]; i++; }
      }
      if (i < line.length) { str += '"'; i++; }
      tokens.push({ type: 'bytes', value: str });
      continue;
    }
    if (line[i] === '"') {
      let str = '"';
      i++;
      while (i < line.length && line[i] !== '"') {
        if (line[i] === '\\' && i + 1 < line.length) { str += line[i] + line[i + 1]; i += 2; }
        else { str += line[i]; i++; }
      }
      if (i < line.length) { str += '"'; i++; }
      tokens.push({ type: 'string', value: str });
      continue;
    }
    if (/[a-zA-Z_]/.test(line[i])) {
      let ident = '';
      while (i < line.length && /[a-zA-Z0-9_]/.test(line[i])) { ident += line[i]; i++; }
      if (KEYWORD_SET.has(ident)) tokens.push({ type: 'keyword', value: ident });
      else if (TYPE_SET.has(ident)) tokens.push({ type: 'type', value: ident });
      else tokens.push({ type: 'ident', value: ident });
      continue;
    }
    const threeChar = line.slice(i, i + 3);
    if (['...'].includes(threeChar)) {
      tokens.push({ type: 'op', value: threeChar }); i += 3; continue;
    }
    const twoChar = line.slice(i, i + 2);
    if (['==', '!=', '<=', '>=', '<<', '>>', '&&', '||', '->', '=>', '+=', '-=', '*=', '/=', '%=', '::', '..'].includes(twoChar)) {
      tokens.push({ type: 'op', value: twoChar }); i += 2; continue;
    }
    if ('+-*/%&|^!~<>=.,:;()[]{}?'.includes(line[i])) {
      tokens.push({ type: 'op', value: line[i] }); i++; continue;
    }
    tokens.push({ type: 'ident', value: line[i] }); i++;
  }
  return tokens;
}

function getColorClass(type: Token['type']): string {
  switch (type) {
    case 'keyword': return 'text-purple-400 font-semibold';
    case 'type': return 'text-cyan-400';
    case 'hex': return 'text-orange-400 font-semibold';
    case 'number': return 'text-orange-300';
    case 'float': return 'text-orange-300';
    case 'typedint': return 'text-orange-300';
    case 'interp': return 'text-green-400';
    case 'string': return 'text-green-400';
    case 'bytes': return 'text-yellow-400';
    case 'comment': return 'text-zinc-600 italic';
    case 'op': return 'text-pink-400';
    case 'ident': return 'text-zinc-200';
    default: return 'text-zinc-300';
  }
}

function renderLine(line: string): React.ReactNode {
  if (line.length === 0) return '\u200B';
  const tokens = tokenizeLine(line);
  return tokens.map((token, i) => (
    <span key={i} className={getColorClass(token.type)}>{token.value}</span>
  ));
}

export default function CodeEditor({ value, onChange, onRun }: CodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLPreElement>(null);
  const lines = value.split('\n');

  // Find/Replace state
  const [findOpen, setFindOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [replaceQuery, setReplaceQuery] = useState('');
  const [matchCount, setMatchCount] = useState(0);
  const [currentMatch, setCurrentMatch] = useState(0);
  const findInputRef = useRef<HTMLInputElement>(null);

  // Autocomplete state
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [suggestionIndex, setSuggestionIndex] = useState(0);
  const [showSuggestions, setShowSuggestions] = useState(false);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const syncScroll = () => {
    if (textareaRef.current && highlightRef.current) {
      highlightRef.current.scrollTop = textareaRef.current.scrollTop;
      highlightRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  };

  // Find/replace logic
  const findMatches = useCallback((query: string) => {
    if (!query) { setMatchCount(0); setCurrentMatch(0); return; }
    let count = 0;
    let pos = 0;
    while ((pos = value.indexOf(query, pos)) !== -1) { count++; pos += query.length; }
    setMatchCount(count);
    setCurrentMatch(count > 0 ? 1 : 0);
  }, [value]);

  useEffect(() => {
    if (findQuery) findMatches(findQuery);
  }, [findQuery, value, findMatches]);

  const findNext = () => {
    if (!findQuery || !textareaRef.current) return;
    const ta = textareaRef.current;
    const start = ta.selectionEnd;
    let pos = value.indexOf(findQuery, start);
    if (pos === -1) pos = value.indexOf(findQuery, 0);
    if (pos !== -1) {
      ta.focus();
      ta.selectionStart = pos;
      ta.selectionEnd = pos + findQuery.length;
      const matches = value.split(findQuery).length - 1;
      const current = value.substring(0, pos).split(findQuery).length;
      setCurrentMatch(current > matches ? 1 : current);
    }
  };

  const findPrev = () => {
    if (!findQuery || !textareaRef.current) return;
    const ta = textareaRef.current;
    const start = ta.selectionStart - 1;
    let pos = value.lastIndexOf(findQuery, start);
    if (pos === -1) pos = value.lastIndexOf(findQuery);
    if (pos !== -1) {
      ta.focus();
      ta.selectionStart = pos;
      ta.selectionEnd = pos + findQuery.length;
    }
  };

  const replaceNext = () => {
    if (!findQuery || !textareaRef.current) return;
    const ta = textareaRef.current;
    const selected = ta.value.substring(ta.selectionStart, ta.selectionEnd);
    if (selected === findQuery) {
      const newValue = value.substring(0, ta.selectionStart) + replaceQuery + value.substring(ta.selectionEnd);
      onChange(newValue);
      setTimeout(() => {
        if (textareaRef.current) {
          const pos = ta.selectionStart + replaceQuery.length;
          textareaRef.current.selectionStart = textareaRef.current.selectionEnd = pos;
        }
      }, 0);
    }
    findNext();
  };

  const replaceAll = () => {
    if (!findQuery) return;
    const newValue = value.split(findQuery).join(replaceQuery);
    onChange(newValue);
    setMatchCount(0);
  };

  // Autocomplete logic
  const updateSuggestions = useCallback(() => {
    if (!textareaRef.current) return;
    const ta = textareaRef.current;
    const pos = ta.selectionStart;
    const before = value.substring(0, pos);
    const wordMatch = before.match(/[a-zA-Z_][a-zA-Z0-9_]*$/);
    if (wordMatch && wordMatch[0].length >= 1) {
      const prefix = wordMatch[0].toLowerCase();
      const matches = ALL_SUGGESTIONS.filter(s => s.toLowerCase().startsWith(prefix)).slice(0, 8);
      if (matches.length > 0 && matches[0] !== prefix) {
        setSuggestions(matches);
        setSuggestionIndex(0);
        setShowSuggestions(true);
        return;
      }
    }
    setShowSuggestions(false);
  }, [value]);

  const insertSuggestion = (suggestion: string) => {
    if (!textareaRef.current) return;
    const ta = textareaRef.current;
    const pos = ta.selectionStart;
    const before = value.substring(0, pos);
    const after = value.substring(pos);
    const wordMatch = before.match(/[a-zA-Z_][a-zA-Z0-9_]*$/);
    if (wordMatch) {
      const newBefore = before.substring(0, before.length - wordMatch[0].length);
      const newValue = newBefore + suggestion + after;
      onChange(newValue);
      setTimeout(() => {
        if (textareaRef.current) {
          const newPos = newBefore.length + suggestion.length;
          textareaRef.current.selectionStart = textareaRef.current.selectionEnd = newPos;
          textareaRef.current.focus();
        }
      }, 0);
    }
    setShowSuggestions(false);
  };

  // Bracket matching
  const getMatchingBracket = (): { start: number; end: number } | null => {
    if (!textareaRef.current) return null;
    const pos = textareaRef.current.selectionStart;
    if (pos >= value.length) return null;
    const char = value[pos];
    if (BRACKETS[char]) {
      const target = BRACKETS[char];
      let depth = 0;
      for (let i = pos + 1; i < value.length; i++) {
        if (value[i] === char) depth++;
        if (value[i] === target) {
          if (depth === 0) return { start: pos, end: i };
          depth--;
        }
      }
    } else if (CLOSING_BRACKETS.has(char)) {
      const openBrackets = Object.entries(BRACKETS).find(([, v]) => v === char)?.[0];
      if (!openBrackets) return null;
      let depth = 0;
      for (let i = pos - 1; i >= 0; i--) {
        if (value[i] === char) depth++;
        if (value[i] === openBrackets) {
          if (depth === 0) return { start: i, end: pos };
          depth--;
        }
      }
    }
    return null;
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Ctrl+F - Find
    if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
      e.preventDefault();
      setFindOpen(true);
      setReplaceOpen(false);
      setTimeout(() => findInputRef.current?.focus(), 0);
      return;
    }
    // Ctrl+H - Replace
    if ((e.ctrlKey || e.metaKey) && e.key === 'h') {
      e.preventDefault();
      setFindOpen(true);
      setReplaceOpen(true);
      setTimeout(() => findInputRef.current?.focus(), 0);
      return;
    }
    // Escape - close find/replace or suggestions
    if (e.key === 'Escape') {
      setFindOpen(false);
      setShowSuggestions(false);
      return;
    }
    // Autocomplete navigation
    if (showSuggestions && suggestions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSuggestionIndex(prev => (prev + 1) % suggestions.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSuggestionIndex(prev => (prev - 1 + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        insertSuggestion(suggestions[suggestionIndex]);
        return;
      }
    }
    if (e.key === 'Tab') {
      e.preventDefault();
      const start = e.currentTarget.selectionStart;
      const end = e.currentTarget.selectionEnd;
      onChange(value.substring(0, start) + '    ' + value.substring(end));
      setTimeout(() => {
        if (textareaRef.current) {
          textareaRef.current.selectionStart = textareaRef.current.selectionEnd = start + 4;
        }
      }, 0);
      return;
    }
    if (e.key === 'Enter') {
      const start = e.currentTarget.selectionStart;
      const before = value.substring(0, start);
      const currentLine = before.split('\n').pop() || '';
      const indent = currentLine.match(/^\s*/)?.[0] || '';
      const lastChar = before.trim().slice(-1);
      if (lastChar === '{') {
        e.preventDefault();
        const insert = '\n' + indent + '    \n' + indent;
        onChange(value.substring(0, start) + insert + value.substring(e.currentTarget.selectionEnd));
        setTimeout(() => {
          if (textareaRef.current) {
            const pos = start + 1 + indent.length + 4;
            textareaRef.current.selectionStart = textareaRef.current.selectionEnd = pos;
          }
        }, 0);
      }
    }
  };

  const handleKeyUp = () => {
    updateSuggestions();
  };

  const bracketMatch = getMatchingBracket();

  // Build highlighted content with search match highlights
  const renderHighlighted = () => {
    return lines.map((line, i) => (
      <div key={i} className="min-h-[1.5rem]">
        {renderLine(line)}
      </div>
    ));
  };

  return (
    <div className="relative flex-1 flex bg-zinc-950 overflow-hidden">
      {/* Line numbers */}
      <div className="flex flex-col py-4 px-3 text-right bg-zinc-950 border-r border-zinc-800 select-none overflow-hidden">
        {lines.map((_, i) => (
          <div key={i} className="text-xs text-zinc-600 leading-6 min-h-[1.5rem]">{i + 1}</div>
        ))}
      </div>

      {/* Editor area */}
      <div className="relative flex-1 overflow-hidden">
        {/* Find/Replace bar */}
        {findOpen && (
          <div className="absolute top-0 right-0 z-20 bg-zinc-800 border-b border-l border-zinc-700 rounded-bl-lg p-2 flex flex-col gap-2 w-80 shadow-xl">
            <div className="flex items-center gap-2">
              <input
                ref={findInputRef}
                type="text"
                value={findQuery}
                onChange={(e) => setFindQuery(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Enter') findNext(); }}
                placeholder="Find..."
                className="flex-1 bg-zinc-900 text-zinc-200 text-xs px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500"
              />
              <span className="text-xs text-zinc-500 min-w-[60px]">
                {matchCount > 0 ? `${currentMatch}/${matchCount}` : '0/0'}
              </span>
              <button onClick={findPrev} className="text-zinc-400 hover:text-zinc-200 text-xs px-1">↑</button>
              <button onClick={findNext} className="text-zinc-400 hover:text-zinc-200 text-xs px-1">↓</button>
              <button onClick={() => { setFindOpen(false); setReplaceOpen(false); }} className="text-zinc-400 hover:text-zinc-200 text-xs px-1">✕</button>
            </div>
            {replaceOpen && (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={replaceQuery}
                  onChange={(e) => setReplaceQuery(e.target.value)}
                  onKeyDown={(e) => { if (e.key === 'Enter') replaceNext(); }}
                  placeholder="Replace..."
                  className="flex-1 bg-zinc-900 text-zinc-200 text-xs px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500"
                />
                <button onClick={replaceNext} className="text-xs text-zinc-300 hover:text-white bg-zinc-700 px-2 py-1 rounded">Replace</button>
                <button onClick={replaceAll} className="text-xs text-zinc-300 hover:text-white bg-zinc-700 px-2 py-1 rounded">All</button>
              </div>
            )}
            {!replaceOpen && (
              <button onClick={() => setReplaceOpen(true)} className="text-xs text-zinc-500 hover:text-zinc-300 text-left">
                Toggle replace...
              </button>
            )}
          </div>
        )}

        {/* Autocomplete dropdown */}
        {showSuggestions && suggestions.length > 0 && (
          <div className="absolute z-20 bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl py-1 max-h-48 overflow-y-auto text-xs">
            {suggestions.map((s, i) => (
              <div
                key={s}
                onClick={() => insertSuggestion(s)}
                className={`px-3 py-1 cursor-pointer ${
                  i === suggestionIndex ? 'bg-emerald-600 text-white' : 'text-zinc-300 hover:bg-zinc-700'
                } ${KEYWORD_SET.has(s) ? 'font-semibold' : ''}`}
              >
                <span className={KEYWORD_SET.has(s) ? 'text-purple-400' : TYPE_SET.has(s) ? 'text-cyan-400' : 'text-zinc-400'}>
                  {s}
                </span>
              </div>
            ))}
          </div>
        )}

        {/* Syntax highlight overlay */}
        <pre
          ref={highlightRef}
          aria-hidden="true"
          className="absolute inset-0 p-4 font-mono text-sm leading-6 pointer-events-none overflow-auto whitespace-pre"
          style={{ margin: 0 }}
        >
          {renderHighlighted()}
          <div className="min-h-[1.5rem]">{'\u200B'}</div>
        </pre>

        {/* Actual textarea */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onKeyUp={handleKeyUp}
          onScroll={syncScroll}
          onClick={() => setShowSuggestions(false)}
          onContextMenu={(e) => {
            e.preventDefault();
            setContextMenu({ x: e.clientX, y: e.clientY });
          }}
          spellCheck={false}
          className="absolute inset-0 p-4 font-mono text-sm leading-6 bg-transparent text-transparent caret-emerald-400 resize-none outline-none whitespace-pre overflow-auto"
          style={{ tabSize: 4 }}
        />
      </div>

      {/* Editor context menu */}
      {contextMenu && (
        <EditorContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          items={[
            {
              label: 'Cut',
              icon: '✂',
              action: () => {
                const ta = textareaRef.current;
                if (ta) {
                  document.execCommand('cut');
                  onChange(ta.value);
                }
              },
            },
            {
              label: 'Copy',
              icon: '📋',
              action: () => {
                const ta = textareaRef.current;
                if (ta) document.execCommand('copy');
              },
            },
            {
              label: 'Paste',
              icon: '📄',
              action: async () => {
                const ta = textareaRef.current;
                if (ta) {
                  const text = await navigator.clipboard.readText();
                  const start = ta.selectionStart;
                  const end = ta.selectionEnd;
                  const newValue = value.substring(0, start) + text + value.substring(end);
                  onChange(newValue);
                  setTimeout(() => {
                    if (textareaRef.current) {
                      const pos = start + text.length;
                      textareaRef.current.selectionStart = textareaRef.current.selectionEnd = pos;
                    }
                  }, 0);
                }
              },
            },
            {
              label: 'Select All',
              icon: '✦',
              action: () => {
                const ta = textareaRef.current;
                if (ta) {
                  ta.focus();
                  ta.selectionStart = 0;
                  ta.selectionEnd = value.length;
                }
              },
            },
            { separator: true },
            {
              label: 'Find...',
              icon: '🔍',
              action: () => {
                setFindOpen(true);
                setReplaceOpen(false);
                setTimeout(() => findInputRef.current?.focus(), 0);
              },
            },
            {
              label: 'Replace...',
              icon: '🔄',
              action: () => {
                setFindOpen(true);
                setReplaceOpen(true);
                setTimeout(() => findInputRef.current?.focus(), 0);
              },
            },
            { separator: true },
            {
              label: 'Run Script',
              icon: '▶',
              action: () => onRun?.(),
            },
          ]}
        />
      )}
    </div>
  );
}