/**
 * Post-process the Codama output so the client stays loadable without a build step.
 *
 * @codama/renderers-js@2.3 emits two shapes that a TypeScript-stripping runtime cannot execute,
 * and neither is configurable on the renderer:
 *
 *   1. Extensionless relative specifiers (`from './claimReceipt'`). Node ESM does not guess
 *      extensions, so importing the package entry throws ERR_MODULE_NOT_FOUND.
 *   2. `export enum`, which is not erasable syntax, so `node --experimental-strip-types` throws
 *      ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX.
 *
 * This runs as part of `npm run codegen`, so `codegen:check` still compares the committed tree
 * against a full regeneration and drift is caught.
 */
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const GENERATED = resolve('clients/js/src/generated');

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(e =>
    e.isDirectory() ? walk(join(dir, e.name)) : join(dir, e.name),
  );
}

function exists(p) {
  try {
    statSync(p);
    return true;
  } catch {
    return false;
  }
}

/** './errors' -> './errors/index.ts' when it is a directory, './x' -> './x.ts' when a file. */
function withExtension(spec, fromFile) {
  if (!spec.startsWith('.') || spec.endsWith('.ts')) return spec;
  const base = resolve(dirname(fromFile), spec);
  if (exists(`${base}.ts`)) return `${spec}.ts`;
  if (exists(join(base, 'index.ts'))) return `${spec}/index.ts`;
  throw new Error(`cannot resolve ${spec} from ${fromFile}`);
}

/**
 * `export enum E { A, B }` -> const object + same-named type.
 *
 * Codama only emits the implicit-value form, so members number from zero in source order. The
 * type alias keeps every existing use site working, since the enum is used as both a value and
 * a return type.
 */
const erasedEnums = new Set();

function eraseEnums(source) {
  return source.replace(
    /export enum (\w+) \{\n([^}]*)\n\}/g,
    (_match, name, body) => {
      const members = body
        .split('\n')
        .map(l => l.trim().replace(/,$/, ''))
        .filter(Boolean);
      for (const m of members) {
        if (!/^\w+$/.test(m)) {
          throw new Error(`enum ${name} member "${m}" has an explicit value; teach the script`);
        }
      }
      erasedEnums.add(name);
      const entries = members.map((m, i) => `  ${m}: ${i},`).join('\n');
      return [
        `export const ${name} = {`,
        entries,
        `} as const;`,
        ``,
        `export type ${name} = (typeof ${name})[keyof typeof ${name}];`,
      ].join('\n');
    },
  );
}

/**
 * `<T>(x) => ...` in expression position is ambiguous with an angle-bracket type assertion, which
 * strip-mode rejects. A trailing comma in the type parameter list disambiguates it and is valid
 * TypeScript. Only expression positions are rewritten: after `(`, `,`, `=`, `=>` or `return`.
 */
function disambiguateGenericArrows(source) {
  return source.replace(
    /(\(|,|=|=>|\breturn)(\s*)<([^<>]+?)>\(/g,
    (match, lead, gap, params) => (params.trimEnd().endsWith(',') ? match : `${lead}${gap}<${params},>(`),
  );
}

/**
 * `<Type>{ ... }` is a real angle-bracket type assertion, which strip-mode rejects outright.
 * Rewritten to the equivalent `{ ... } as Type` by matching the object literal's closing brace,
 * skipping braces that appear inside string or template literals.
 */
function rewriteAngleBracketAssertions(source) {
  let out = source;
  for (;;) {
    const m = /<([A-Za-z_$][\w$.]*)>\{/.exec(out);
    if (!m) return out;
    const open = m.index + m[0].length - 1;
    let depth = 0;
    let quote = null;
    let end = -1;
    for (let i = open; i < out.length; i += 1) {
      const c = out[i];
      if (quote) {
        if (c === '\\') i += 1;
        else if (c === quote) quote = null;
        continue;
      }
      if (c === "'" || c === '"' || c === '`') quote = c;
      else if (c === '{') depth += 1;
      else if (c === '}') {
        depth -= 1;
        if (depth === 0) {
          end = i;
          break;
        }
      }
    }
    if (end < 0) throw new Error(`unbalanced object literal after <${m[1]}>`);
    out =
      out.slice(0, m.index) +
      out.slice(open, end + 1) +
      ` as ${m[1]}` +
      out.slice(end + 1);
  }
}

/**
 * A TS enum member doubles as a type (`E.A` is the literal type of that member); a const object
 * member does not. Only the type-position uses need `typeof E.A`, so the rewrite is scoped to
 * `export type Parsed...Instruction<...> = <union>;` and never touches the value-position uses in
 * the parser below it, where `typeof` would mean the JavaScript operator.
 */
function qualifyErasedEnumTypeUses(source) {
  let out = source;
  for (const name of erasedEnums) {
    out = out.replace(
      /export type Parsed\w+Instruction<[\s\S]*?(?=\nexport )/,
      block =>
        block.replace(
          new RegExp(`(instructionType:\\s*)${name}\\.`, 'g'),
          (_m, key) => `${key}typeof ${name}.`,
        ),
    );
  }
  return out;
}

let changed = 0;
for (const file of walk(GENERATED).filter(f => f.endsWith('.ts'))) {
  const before = readFileSync(file, 'utf8');
  let after = before.replace(
    /(\bfrom\s+')(\.[^']*)(')/g,
    (_m, a, spec, b) => `${a}${withExtension(spec, file)}${b}`,
  );
  after = eraseEnums(after);
  after = disambiguateGenericArrows(after);
  after = rewriteAngleBracketAssertions(after);
  after = qualifyErasedEnumTypeUses(after);
  if (after !== before) {
    writeFileSync(file, after);
    changed += 1;
  }
}
// The guard lives here rather than only in CI: if a Codama upgrade emits some other
// non-erasable construct, codegen fails loudly instead of shipping a package that cannot load.
const { stripTypeScriptTypes } = await import('node:module');
const broken = [];
for (const file of walk(GENERATED).filter(f => f.endsWith('.ts'))) {
  try {
    stripTypeScriptTypes(readFileSync(file, 'utf8'), { mode: 'strip' });
  } catch (e) {
    broken.push(`${file}: ${String(e.message).split('\n')[0]}`);
  }
}
if (broken.length > 0) {
  console.error('generated output is not erasable TypeScript:\n  ' + broken.join('\n  '));
  process.exit(1);
}

console.log(`codegen-postprocess: rewrote ${changed} generated file(s), all erasable`);
