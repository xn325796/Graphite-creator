import { vanillaNames } from './schema.js'


const NAME_RE = /^[a-z0-9]+(-[a-z0-9]+)*$/

const MSG = {
  'meta.name': [['模组 meta 缺少 name', 'Mod meta is missing name'], 'error'],
  'meta.version': [['minGameVersion 缺失或非法（当前：{v}）', 'minGameVersion missing or invalid (got: {v})'], 'error'],
  'record.name': [['记录缺少 name', 'Record is missing name'], 'error'],
  'record.nameChars': [['名称「{name}」含非法字符（只允许小写字母/数字/连字符）', 'Name "{name}" has invalid characters (lowercase/digits/hyphens only)'], 'error'],
  'record.duplicate': [['同类型下名称重复：{name}', 'Duplicate name within type: {name}'], 'error'],
  'ref.missing': [['字段 {field} 引用的内容不存在：{ref}', 'Field {field} references missing content: {ref}'], 'error'],
  'strings.missing': [['字符串 {key} 缺少 {lang} 翻译', 'String {key} is missing {lang} translation'], 'warn'],
}

export function issueText(issue, lang) {
  if (issue.message) return issue.message
  const m = MSG[issue.key]
  if (!m) return issue.key
  let s = m[0][lang === 'en' ? 1 : 0]
  for (const [k, v] of Object.entries(issue.args || {})) s = s.replaceAll(`{${k}}`, String(v))
  return s
}

export function issueLevel(key) {
  return MSG[key] ? MSG[key][1] : 'warn'
}

function mk(key, args, contentId) {
  return { level: issueLevel(key), key, args, contentId }
}

function knownContent(project, type) {
  const set = new Set(vanillaNames[type] || [])
  for (const r of project.records) if (r.type === type) set.add(r.name)
  return set
}

export function validateProject(project) {
  const issues = []
  if (!project) return issues

  if (!project.meta.name || !project.meta.name.trim()) issues.push(mk('meta.name', {}))
  const v = project.meta.minGameVersion
  if (v == null || !/^\d+(\.\d+)*$/.test(String(v))) issues.push(mk('meta.version', { v: v ?? '∅' }))

  const seen = new Map()
  for (const r of project.records) {
    const id = `${r.type}/${r.name}`
    if (!r.name) {
      issues.push(mk('record.name', {}, id))
      continue
    }
    if (!NAME_RE.test(r.name)) issues.push(mk('record.nameChars', { name: r.name }, id))
    const dk = `${r.type}:${r.name}`
    if (seen.has(dk)) issues.push(mk('record.duplicate', { name: r.name }, id))
    seen.set(dk, true)
  }

  const REF_FIELDS = { research: null, effect: 'status', status: 'status' }
  for (const r of project.records) {
    for (const [field, refType] of Object.entries(REF_FIELDS)) {
      const ref = r.fields[field]
      if (ref == null || ref === '') continue
      const pool = refType ? knownContent(project, refType) : new Set(Object.keys(vanillaNames).flatMap((tp) => [...(vanillaNames[tp] || [])]).concat(project.records.map((x) => x.name)))
      if (!pool.has(String(ref))) issues.push(mk('ref.missing', { field, ref }, `${r.type}/${r.name}`))
    }
  }

  for (const [key, langs] of Object.entries(project.strings)) {
    for (const lang of ['zh', 'en']) {
      if (!langs[lang] || !String(langs[lang]).trim()) {
        issues.push(mk('strings.missing', { key, lang: lang === 'zh' ? '简体中文' : 'English' }, key.split('.').slice(0, 2).join('/')))
      }
    }
  }
  return issues
}
