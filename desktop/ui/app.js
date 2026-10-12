// KeyZapper UI: renders the view the Rust side sends and calls its commands. No framework, no build step.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let view = null;
let english = {};
/** Per profile: { checking, result: { success, message } } */
const checks = new Map();
/** Open dialogs that re-render when the view changes. */
const liveDialogs = new Set();
let alertOpen = false;
let whatsNewShown = false;

// MARK: Helpers

/** Translates a German source string and fills its `%@` placeholders in order. */
function t(key, ...args) {
  const text = view?.language === 'en' ? english[key] ?? key : key;
  let i = 0;
  return text.replace(/%@/g, () => (i < args.length ? String(args[i++]) : '%@'));
}

const PROPS = new Set(['value', 'checked', 'disabled', 'selected', 'hidden', 'required']);

function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props ?? {})) {
    if (value == null || value === false) continue;
    if (key.startsWith('on')) el.addEventListener(key.slice(2), value);
    else if (key === 'class') el.className = value;
    else if (PROPS.has(key)) el[key] = value;
    else el.setAttribute(key, value === true ? '' : value);
  }
  for (const child of children.flat(Infinity)) {
    if (child == null || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

const ICONS = {
  plus: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  folderPlus: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 11v6M9 14h6"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  shield: '<path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z"/><path d="m9 12 2 2 4-4"/>',
  archive: '<rect x="3" y="4" width="18" height="4" rx="1"/><path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>',
  key: '<circle cx="7.5" cy="15.5" r="4.5"/><path d="m10.7 12.3 9.3-9.3M17 6l3 3M14 9l2 2"/>',
  keyOff: '<circle cx="7.5" cy="15.5" r="4.5"/><path d="m10.7 12.3 9.3-9.3M17 6l3 3M3 3l18 18"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1"/>',
  pencil: '<path d="M17 3l4 4L8 20H4v-4z"/>',
  rotate: '<path d="M3 12a9 9 0 0 1 15-6.7L21 8M21 3v5h-5M21 12a9 9 0 0 1-15 6.7L3 16M3 21v-5h5"/>',
  trash: '<path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14"/>',
  check: '<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>',
  warning: '<path d="M12 3 2 20h20z"/><path d="M12 9v5M12 17h.01"/>',
  error: '<path d="M8 2h8l6 6v8l-6 6H8l-6-6V8z"/><path d="m15 9-6 6M9 9l6 6"/>',
  question: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.7M12 17h.01"/>',
  flame: '<path d="M12 22c4 0 7-3 7-7 0-4-3-6-4-9-1 2-2 3-3 3 0-2-1-4-3-6 0 4-4 6-4 12 0 4 3 7 7 7z"/>',
  dollar: '<circle cx="12" cy="12" r="9"/><path d="M15 9h-4a1.5 1.5 0 0 0 0 3h2a1.5 1.5 0 0 1 0 3H9M12 7v2M12 15v2"/>',
  pause: '<circle cx="12" cy="12" r="9"/><path d="M10 9v6M14 9v6"/>',
  turn: '<path d="M5 4v7a3 3 0 0 0 3 3h11"/><path d="m15 10 4 4-4 4"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  bolt: '<path d="M13 2 4 14h7l-1 8 9-12h-7z"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>',
  server: '<rect x="3" y="4" width="18" height="7" rx="1"/><rect x="3" y="13" width="18" height="7" rx="1"/><path d="M7 7.5h.01M7 16.5h.01"/>',
  fileLock: '<path d="M6 2h8l4 4v16H6z"/><path d="M14 2v4h4"/><rect x="9" y="13" width="6" height="5" rx="1"/><path d="M10 13v-2a2 2 0 0 1 4 0v2"/>',
  arrow: '<path d="M5 12h14M13 6l6 6-6 6"/>',
  sparkles: '<path d="M10 3l1.8 4.7 4.7 1.8-4.7 1.8L10 16l-1.8-4.7L3.5 9.5l4.7-1.8z"/><path d="M18 14l.9 2.1 2.1.9-2.1.9L18 20l-.9-2.1-2.1-.9 2.1-.9z"/>',
};

function icon(name, cls = '') {
  const span = document.createElement('span');
  span.innerHTML = `<svg class="icon ${cls}" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]}</svg>`;
  return span.firstElementChild;
}

function button(label, onClick, { iconName, cls, disabled, title, iconOnly } = {}) {
  return h('button', {
    type: 'button', class: [cls, iconOnly && 'icon-only'].filter(Boolean).join(' ') || null, disabled,
    title: title ?? (iconOnly ? label : null), 'aria-label': iconOnly ? label : null, onclick: onClick,
  }, iconName && icon(iconName), !iconOnly && label);
}

async function call(command, args) {
  try {
    const result = await invoke(command, args);
    if (result?.profiles) setView(result);
    else if (Array.isArray(result) && result[1]?.profiles) setView(result[1]);
    else setView(await invoke('get_view'));
    return result;
  } catch (error) {
    showAlert(String(error));
    return null;
  }
}

const isMac = () => view?.platform !== 'windows';
const revealLabel = () => (isMac() ? t('Im Finder zeigen') : t('Im Explorer zeigen'));
const profileName = (id) => view.profiles.find((p) => p.id === id)?.name ?? t('Unbekanntes Profil');
const credentialOk = (p) => (p.sso ? p.ssoState === 'loggedIn' : p.hasKey);
const ssoStateText = (p) => ({ loggedIn: t('Angemeldet'), expired: t('Sitzung abgelaufen') }[p.ssoState] ?? t('Nicht angemeldet'));
/** Picker label: SSO profiles carry a badge and their login state. */
const profileLabel = (p) => (p.sso ? `${p.name} · SSO (${ssoStateText(p)})` : p.name);
/** Signs an SSO profile in before it is used; false if it is (still) not signed in. */
async function ensureLogin(profileId) {
  const profile = view.profiles.find((p) => p.id === profileId);
  if (!profile?.sso || profile.ssoState === 'loggedIn') return true;
  await call('sso_login', { profileId });
  return view.profiles.find((p) => p.id === profileId)?.ssoState === 'loggedIn';
}
const locale = () => (navigator.language?.toLowerCase().startsWith(view.language) ? navigator.language : view.language);

// MARK: Render

function setView(next) {
  view = next;
  document.documentElement.lang = view.language;
  renderToolbar();
  renderBanners();
  renderMain();
  for (const render of liveDialogs) render();
  if (view.error && !alertOpen) showAlert(view.error, () => call('dismiss_error'));
  if (view.whatsNew.length && !whatsNewShown) whatsNewDialog();
}

let toolbarSignature = '';

function renderToolbar() {
  const signature = [view.language, view.profiles.length > 0, view.disabled, view.platform].join('|');
  if (signature === toolbarSignature) return;
  toolbarSignature = signature;
  const backupMenu = h('div', { id: 'backup-menu', popover: 'auto', class: 'menu' },
    button(t('Backup exportieren …'), () => { backupMenu.hidePopover(); exportDialog(); }, { disabled: !view.profiles.length }),
    button(t('Backup importieren …'), () => { backupMenu.hidePopover(); importDialog(); }));
  const backupButton = button(t('Backup'), null, { iconName: 'archive', iconOnly: true, title: t('Backup exportieren oder importieren') });
  backupButton.setAttribute('popovertarget', 'backup-menu');
  backupMenu.addEventListener('toggle', (event) => {
    if (event.newState !== 'open') return;
    const rect = backupButton.getBoundingClientRect();
    backupMenu.style.position = 'fixed';
    backupMenu.style.inset = 'auto';
    backupMenu.style.top = `${rect.bottom + 4}px`;
    backupMenu.style.left = `${Math.max(8, rect.right - backupMenu.offsetWidth)}px`;
  });
  const active = h('input', {
    type: 'checkbox', role: 'switch', checked: !view.disabled,
    onchange: async (event) => {
      if (event.target.checked) return call('set_disabled', { disabled: false });
      event.target.checked = true;
      if (await confirmDialog(t('KeyZapper deaktivieren?'),
        t('KeyZapper entfernt seine Einträge aus allen Projekten und aus ~/.claude/settings.json. Die Zuordnungen bleiben gespeichert und werden beim Aktivieren wieder geschrieben.'),
        t('Deaktivieren'))) call('set_disabled', { disabled: true });
    },
  });
  document.getElementById('toolbar').replaceChildren(
    h('h1', null, 'KeyZapper'),
    h('span', { class: 'spacer' }),
    button(t('Neues Profil'), () => profileEditor(null), { iconName: 'plus' }),
    button(t('Projekt zuordnen'), () => addProjectDialog(null), { iconName: 'folderPlus', disabled: !view.profiles.length }),
    button(t('Status aktualisieren'), () => call('refresh_all'), { iconName: 'refresh', iconOnly: true }),
    button(t('Claude-Einstellungen prüfen'), () => { call('refresh_all'); claudeSettingsDialog(); },
      { iconName: 'shield', iconOnly: true, title: t('~/.claude/settings.json prüfen, reparieren und Standardprofil setzen') }),
    backupButton,
    backupMenu,
    h('label', { class: 'switch', title: t('KeyZapper vorübergehend deaktivieren: Projekte nutzen dann die normale Claude-Anmeldung') },
      active, t('KeyZapper aktiv')),
    button(t('So funktioniert’s'), howItWorksDialog, { iconName: 'info', iconOnly: true }),
  );
}

function banner(kind, title, detail, actions = [], onDismiss = null) {
  return h('div', { class: `banner ${kind}`, role: kind === 'warning' ? 'alert' : 'status' },
    icon(kind === 'warning' ? 'warning' : 'info'),
    h('div', { class: 'banner-text' },
      h('div', { class: `banner-title ${kind}` }, title),
      detail && h('div', { class: 'detail' }, detail)),
    actions.map(([label, action]) => button(label, action)),
    onDismiss && button(t('Schließen'), onDismiss, { iconName: 'close', iconOnly: true, cls: 'plain' }));
}

function renderBanners() {
  const items = [];
  if (view.availableBackup) {
    items.push(banner('info',
      t('OneDrive-Backup gefunden: %@ Profil(e), %@ Projekt(e)', view.availableBackup.profiles, view.availableBackup.projects),
      t('Profile und Zuordnungen wiederherstellen? Keys sind nicht im Backup und müssen neu eingetragen werden.'),
      [[t('Wiederherstellen'), () => call('restore_from_backup')], [t('Verwerfen'), () => call('discard_backup')]]));
  }
  if (view.backupProblem) items.push(banner('warning', view.backupProblem));
  if (view.keychainKeys > 0) {
    items.push(banner('warning',
      t('%@ Key(s) liegen noch im macOS-Schlüsselbund', view.keychainKeys),
      t('KeyZapper speichert Keys jetzt ohne Schlüsselbund. Einmal übernehmen; macOS fragt dabei je Key nach der Erlaubnis.'),
      [[t('Übernehmen'), () => call('migrate_keychain')]]));
  }
  if (view.disabled) {
    items.push(banner('warning', t('KeyZapper ist deaktiviert'),
      t('Claude Code nutzt in allen Projekten die normale Anmeldung. Die Zuordnungen bleiben gespeichert.'),
      [[t('Aktivieren'), () => call('set_disabled', { disabled: false })]]));
  }
  const problems = view.globalFindings.filter((f) => f.severity !== 'info');
  if (problems.length) {
    items.push(banner('warning', t('~/.claude/settings.json: %@ Hinweis(e)', problems.length), problems[0].message,
      [[t('Prüfen'), claudeSettingsDialog]]));
  }
  if (view.outdatedClis.length) {
    items.push(banner('warning', t('Claude-Code-CLI veraltet: %@', view.outdatedClis.join(', ')),
      t('IntelliJ nutzt diese CLI. Unter %@ gilt die Projektzuordnung nur beim Start direkt im Projektordner. Aktualisieren mit „claude update“.', view.minimumCliVersion)));
  }
  if (view.availableUpdate) {
    const { version, prerelease } = view.availableUpdate;
    items.push(banner('info',
      prerelease ? t('Beta: KeyZapper %@ ist verfügbar (installiert: %@)', version, view.appVersion ?? '?')
        : t('KeyZapper %@ ist verfügbar (installiert: %@)', version, view.appVersion ?? '?'),
      view.installingUpdate ? t('Update wird geladen, geprüft und installiert – KeyZapper startet danach neu …')
        : prerelease ? t('Beta-Versionen sind zum Testen gedacht. Die Installation benötigt Administratorrechte, danach startet KeyZapper neu.')
          : t('Die Installation benötigt Administratorrechte, danach startet KeyZapper neu.'),
      [[t('Installieren'), () => call('install_update')], [t('Versionshinweise'), () => call('open_url', { url: view.availableUpdate.pageUrl })]]));
  }
  if (view.notice) items.push(banner('info', view.notice, null, [], () => call('dismiss_notice')));
  document.getElementById('banners').replaceChildren(...items);
}

function renderMain() {
  const main = document.getElementById('main');
  if (!view.profiles.length) {
    main.replaceChildren(h('div', { class: 'empty' },
      icon('key'),
      h('h2', null, t('Noch keine Profile')),
      h('p', null, t('Lege ein Profil mit LiteLLM-Endpunkt, Modellalias und deinem freigegebenen Key an.')),
      button(t('Profil anlegen'), () => profileEditor(null), { cls: 'primary' }),
      button(t('Backup importieren …'), importDialog, { cls: 'link' })),
      footer());
    return;
  }
  main.replaceChildren(
    ...view.profiles.map(profileCard),
    h('div', { class: 'hint' }, icon('bolt', 'accent'),
      t('Claude Code holt in jedem zugeordneten Projekt den passenden Key automatisch über den Helper – kein manuelles Wechseln nötig. Nach neuen Zuordnungen die Claude-Sitzung einmal neu starten.')),
    footer());
}

function footer() {
  // The result shows as a banner at the top, out of sight when the footer was clicked at the bottom.
  const checkUpdates = async (beta) => {
    await call('check_for_updates', { beta });
    window.scrollTo(0, 0);
  };
  return h('div', { class: 'footer' },
    view.appVersion ? `KeyZapper ${view.appVersion}` : t('KeyZapper Entwicklungsversion'),
    view.updateCheckEnabled
      ? [button(t('Nach Updates suchen'), () => checkUpdates(false), { cls: 'link' }),
        button(t('Beta-Version suchen'), () => checkUpdates(true), { cls: 'link' })]
      : h('span', null, t('Updates über die IT')),
    button(t('So funktioniert’s'), howItWorksDialog, { cls: 'link' }),
    button(t('Projektseite'), () => call('open_url', { url: view.projectUrl }), { cls: 'link' }));
}

function tierModels(p) {
  return [['Opus', p.opusModel], ['Sonnet', p.sonnetModel], ['Haiku', p.haikuModel]]
    .filter(([, model]) => model).map(([tier, model]) => `${tier} ${model}`).join(' · ');
}

function money(value) {
  return new Intl.NumberFormat(locale(), { style: 'currency', currency: 'USD' }).format(value);
}

/** LiteLLM writes Python ISO dates, e.g. `2026-11-01T00:00:00.123456+00:00`, sometimes without zone (UTC). */
function parseResetDate(text) {
  let value = text.replace(/\.\d+/, '');
  if (!/(Z|[+-]\d\d:?\d\d)$/.test(value)) value += 'Z';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

function budgetLine(budget) {
  const reset = budget.resetAt && parseResetDate(budget.resetAt);
  return h('div', { class: 'line' }, icon('dollar', 'muted'),
    budget.maxBudget != null
      ? [h('strong', { class: budget.remaining > 0 ? 'green' : 'red' }, t('%@ übrig', money(budget.remaining))),
        h('span', { class: 'muted' }, t('von %@', money(budget.maxBudget)))]
      : h('strong', { class: 'green' }, t('Kein Budgetlimit')),
    h('span', { class: 'muted' }, '·'),
    h('span', { class: 'red' }, t('%@ verbraucht', money(budget.spend))),
    reset && h('span', { class: 'muted' }, t('· Reset %@', new Intl.DateTimeFormat(locale(), { dateStyle: 'medium' }).format(reset))));
}

function profileCard(profile) {
  const bindings = view.bindings.filter((b) => b.profileId === profile.id);
  const check = checks.get(profile.id) ?? {};
  const keyColor = credentialOk(profile) ? 'var(--green)' : 'var(--red)';
  const testConnection = async () => {
    checks.set(profile.id, { checking: true });
    renderMain();
    const result = await call('test_connection', { profileId: profile.id });
    checks.set(profile.id, { result });
    renderMain();
  };
  const projectsTitle = !bindings.length ? t('Noch keinem Projekt zugeordnet')
    : bindings.length === 1 ? t('Nutzen diesen Key · 1 Projekt') : t('Nutzen diesen Key · %@ Projekte', bindings.length);

  return h('section', { class: 'card', 'aria-label': profile.name },
    h('div', { class: 'card-head' },
      h('div', { class: 'spacer' },
        h('h2', null, profile.name, profile.sso && h('span', { class: 'badge' }, 'SSO'), profile.managed && h('span', { class: 'badge' }, t('Von der IT vorgegeben'))),
        h('div', { class: 'muted selectable' }, profile.endpoint + (profile.modelAlias ? t(' · Modell %@', profile.modelAlias) : '')),
        tierModels(profile) && h('div', { class: 'mono muted' }, tierModels(profile))),
      button(t('Bearbeiten'), () => profileEditor(profile), { disabled: profile.managed }),
      button(t('Löschen'), () => deleteProfile(profile, bindings), { cls: 'danger', disabled: profile.managed })),
    profile.sso ? ssoRow(profile, testConnection, check) : h('div', { class: 'key-row' },
      h('span', { class: `chip ${profile.hasKey ? 'ok' : 'missing'}` }, icon(profile.hasKey ? 'key' : 'keyOff'),
        h('span', { class: profile.hasKey ? 'mono' : null }, profile.hasKey ? `Key ${profile.keyHint ?? '••••'}` : t('Kein Key hinterlegt – Anfragen schlagen fehl'))),
      h('span', { class: 'spacer' }),
      button(t('Kopieren'), () => call('copy_key', { profileId: profile.id }),
        { iconName: 'copy', disabled: !profile.hasKey || !view.allowKeyExport, title: view.allowKeyExport ? null : t('Laut Firmenrichtlinie deaktiviert') }),
      button(profile.hasKey ? t('Ändern') : t('Key hinterlegen'), () => replaceKeyDialog(profile), { iconName: 'pencil' }),
      button(t('Verbindung prüfen'), testConnection, { disabled: check.checking || !profile.hasKey })),
    profile.hasKey && profile.budget && budgetLine(profile.budget),
    profile.hasKey && profile.killers.length > 0 && h('div', { class: 'line orange' }, icon('flame'),
      t('Wird vom Budget-Killer in „%@“ verbrannt', profile.killers.join(', '))),
    check.checking && h('div', { class: 'muted' }, t('Verbindung wird geprüft …')),
    check.result && h('div', { class: `check-result ${check.result.success ? 'green' : 'orange'}` },
      icon(check.result.success ? 'check' : 'warning'), h('span', { class: 'selectable' }, check.result.message)),
    h('div', { class: 'projects', style: `--connector: color-mix(in srgb, ${keyColor} 45%, transparent)` },
      h('div', { class: 'projects-head' }, icon('turn'), h('span', { class: 'spacer' }, projectsTitle),
        button(t('Projekt zuordnen …'), () => addProjectDialog(profile.id), { iconName: 'folderPlus' })),
      bindings.length > 0 && h('div', { class: 'project-list' }, bindings.map((b) => projectRow(b, profile)))));
}

function ssoRow(profile, testConnection, check) {
  const expiry = profile.ssoExpiresAt && profile.ssoExpiresAt * 1000 > Date.now()
    ? new Date(profile.ssoExpiresAt * 1000).toLocaleString(locale()) : null;
  const text = profile.ssoLoggingIn ? t('Anmeldung läuft – bitte im Browser abschließen …')
    : profile.ssoState === 'loggedIn' ? (expiry ? t('Angemeldet, Token gültig bis %@', expiry) : t('Angemeldet'))
      : ssoStateText(profile);
  const ok = profile.ssoState === 'loggedIn';
  return h('div', { class: 'key-row' },
    h('span', { class: `chip ${ok ? 'ok' : 'missing'}` }, icon(ok ? 'key' : 'keyOff'), h('span', null, text)),
    profile.ssoLoggingIn && h('span', { class: 'spinner', role: 'status', 'aria-label': t('Anmeldung läuft') }),
    h('span', { class: 'spacer' }),
    button(ok || profile.ssoState === 'expired' ? t('Neu anmelden') : t('Anmelden'), () => call('sso_login', { profileId: profile.id }),
      { iconName: 'key', cls: ok ? null : 'primary', disabled: profile.ssoLoggingIn }),
    button(t('Abmelden'), () => call('sso_logout', { profileId: profile.id }), { disabled: profile.ssoLoggingIn || profile.ssoState === 'loggedOut' }),
    button(t('Verbindung prüfen'), testConnection, { disabled: check.checking || !ok }));
}

function statusAppearance(status, keyPresent) {
  if (!status) return ['question', 'muted'];
  if (status.health.state === 'folderMissing' || !keyPresent || status.conflicts.some((c) => c.blocking)) return ['error', 'red'];
  if (status.health.state !== 'active') return ['warning', 'orange'];
  return ['check', 'green'];
}

function describeHealth(health, keyPresent, sso) {
  if (!keyPresent && health?.state === 'active' && sso) return t('Nicht angemeldet – Claude-Anfragen schlagen fehl. Beim Profil „Anmelden“ wählen.');
  if (!keyPresent && health?.state === 'active') return t('Für das Profil ist kein Key hinterlegt – Claude-Anfragen schlagen fehl.');
  switch (health?.state) {
    case 'active': return t('Aktiv');
    case 'notApplied': return t('Nicht eingerichtet – „Erneut anwenden“ wählen.');
    case 'drifted': return t('Abweichung in %@ – „Erneut anwenden“ wählen.', health.keys.join(', '));
    case 'folderMissing': return t('Ordner nicht gefunden (verschoben oder gelöscht). Zuordnung entfernen und neu zuordnen.');
    default: return t('Status unbekannt');
  }
}

function projectRow(binding, profile) {
  const status = binding.status;
  const keyPresent = credentialOk(profile);
  const [symbol, color] = statusAppearance(status, keyPresent);
  const healthOk = status?.health.state === 'active' && keyPresent;
  const statusTip = view.disabled ? t('Deaktiviert – Claude nutzt hier die normale Anmeldung.')
    : !healthOk ? describeHealth(status?.health, keyPresent, profile.sso) : status.conflicts[0]?.message ?? t('Aktiv');
  // Hidden: three clicks on the status icon switch the Budget-Killer on or off.
  const statusIcon = view.disabled ? h('span', { class: 'status-icon muted', title: statusTip }, icon('pause'))
    : h('span', { class: `status-icon ${color}`, title: statusTip, onclick: (e) => { if (e.detail === 3 && !profile.sso) call('toggle_pool', { bindingId: binding.id }); } }, icon(symbol));
  const select = h('select', {
    'aria-label': t('Profil'), title: t('Profil für dieses Projekt wechseln'), disabled: view.disabled,
    onchange: async (e) => {
      if (await ensureLogin(e.target.value)) call('bind', { folder: binding.path, profileId: e.target.value });
      else renderMain();
    },
  }, view.profiles.map((p) => h('option', { value: p.id, selected: p.id === binding.profileId }, profileLabel(p))));
  const confirmUnbind = async () => {
    if (await confirmDialog(t('Zuordnung für „%@“ entfernen?', binding.name),
      t('Nur die von der App gesetzten Einträge werden aus .claude/settings.local.json entfernt. Andere Einstellungen bleiben erhalten.'),
      t('Entfernen'))) call('unbind', { bindingId: binding.id });
  };
  return h('div', { class: 'project' },
    h('div', { class: 'project-main' }, statusIcon,
      h('div', { class: 'project-name' }, h('strong', null, binding.name), h('span', { class: 'mono', title: binding.path }, `\u200E${binding.displayPath}\u200E`)),
      select,
      button(t('Für Agenten optimieren'), () => agentsDialog(binding, profile),
        { iconName: 'sparkles', iconOnly: true, title: t('AGENTS.md für dieses Repository per KI-Analyse erzeugen oder verbessern'), disabled: status?.health.state === 'folderMissing' }),
      button(revealLabel(), () => call('reveal', { path: binding.path }), { iconName: 'folder', iconOnly: true }),
      button(t('Erneut anwenden'), () => call('reapply', { bindingId: binding.id }),
        { iconName: 'rotate', iconOnly: true, title: t('Einstellungen erneut schreiben'), disabled: status?.health.state === 'folderMissing' || view.disabled }),
      button(t('Entfernen'), confirmUnbind, { iconName: 'trash', iconOnly: true, title: t('Zuordnung entfernen'), cls: 'danger' })),
    binding.pooled && !view.disabled && h('div', { class: 'note red' }, icon('flame'),
      t('Budget-Killer: Nach dem Budget dieses Keys werden die Keys der anderen Profile am selben Gateway verbrannt.')),
    view.disabled ? h('div', { class: 'note muted' }, t('Deaktiviert – Claude nutzt hier die normale Anmeldung.'))
      : !healthOk && h('div', { class: `note ${color}` }, describeHealth(status?.health, keyPresent, profile.sso)),
    (status?.conflicts ?? []).map((c) => h('div', { class: `note ${c.blocking ? 'red' : 'orange'}` },
      icon(c.blocking ? 'error' : 'warning'), h('span', { class: 'selectable' }, c.message))));
}

async function deleteProfile(profile, bindings) {
  const message = profile.sso
    ? (bindings.length ? t('Die SSO-Anmeldung wird entfernt und %@ Projektzuordnung(en) werden zurückgenommen.', bindings.length) : t('Die SSO-Anmeldung wird entfernt.'))
    : bindings.length
      ? t('Der Key wird gelöscht und %@ Projektzuordnung(en) werden zurückgenommen.', bindings.length)
      : t('Der Key wird gelöscht.');
  if (await confirmDialog(t('Profil „%@“ löschen?', profile.name), message, t('Löschen'))) {
    checks.delete(profile.id);
    call('delete_profile', { profileId: profile.id });
  }
}

// MARK: Dialogs

/** Opens a modal dialog; `build(close)` returns { body, actions }. Re-rendered on view changes if `live`. */
function openDialog(title, build, { live = false, wide = false } = {}) {
  const dialog = h('dialog', { 'aria-label': title, style: wide ? 'width: min(620px, calc(100vw - 32px))' : null });
  const close = () => dialog.close();
  const render = () => {
    const focused = document.activeElement?.id;
    const { body, actions } = build(close);
    dialog.replaceChildren(h('form', { method: 'dialog', onsubmit: (e) => e.preventDefault() },
      h('div', { class: 'dialog-body' }, body), h('div', { class: 'dialog-actions' }, actions)));
    if (focused) document.getElementById(focused)?.focus();
  };
  render();
  if (live) liveDialogs.add(render);
  dialog.addEventListener('close', () => { liveDialogs.delete(render); dialog.remove(); });
  document.body.append(dialog);
  dialog.showModal();
  return { dialog, render, close };
}

function showAlert(message, onClose) {
  alertOpen = true;
  const { dialog } = openDialog(t('Fehler'), (close) => ({
    body: [h('h2', null, t('Fehler')), h('div', { class: 'alert-message selectable' }, message)],
    actions: [button('OK', close, { cls: 'primary' })],
  }));
  dialog.setAttribute('role', 'alertdialog');
  dialog.addEventListener('close', () => { alertOpen = false; onClose?.(); });
}

function confirmDialog(title, message, confirmLabel) {
  return new Promise((resolve) => {
    let confirmed = false;
    const { dialog } = openDialog(title, (close) => ({
      body: [h('h2', null, title), h('p', { class: 'muted', style: 'margin:0' }, message)],
      actions: [button(t('Abbrechen'), close), button(confirmLabel, () => { confirmed = true; close(); }, { cls: 'primary danger' })],
    }));
    dialog.addEventListener('close', () => resolve(confirmed));
  });
}

function field(label, control, id) {
  control.id = id;
  return h('div', { class: 'field' }, h('label', { for: id }, label), control);
}

function input(props, onInput) {
  return h('input', { type: 'text', autocomplete: 'off', spellcheck: 'false', ...props, oninput: (e) => onInput(e.target.value) });
}

const RESERVED = ['ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL', 'ANTHROPIC_MODEL', 'ANTHROPIC_DEFAULT_OPUS_MODEL',
  'ANTHROPIC_DEFAULT_SONNET_MODEL', 'ANTHROPIC_DEFAULT_HAIKU_MODEL', 'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX', 'CLAUDE_CODE_USE_FOUNDRY'];

/** Lines of the environment text that are invalid or set reserved names. */
function invalidEnvironmentLines(text) {
  return text.split(/\r?\n/).map((l) => l.trim()).filter((line) => {
    if (!line || line.startsWith('#')) return false;
    const i = line.indexOf('=');
    const name = i < 0 ? '' : line.slice(0, i).trim();
    return i < 0 || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) || RESERVED.includes(name);
  });
}

function endpointHost(text) {
  try {
    const url = new URL(text.trim());
    const loopback = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname);
    return (url.protocol === 'https:' || (url.protocol === 'http:' && loopback)) && url.hostname ? url.hostname.toLowerCase() : null;
  } catch { return null; }
}

function isHostAllowed(host) {
  if (!view.allowedHosts.length) return true;
  return view.allowedHosts.some((p) => (p.startsWith('*.') ? host.endsWith(p.slice(1)) : host === p));
}

function profileEditor(existing) {
  const s = {
    name: existing?.name ?? '', endpoint: existing?.endpoint ?? view.defaultEndpoint ?? '',
    modelAlias: existing?.modelAlias ?? view.defaultModelAlias ?? '', opusModel: existing?.opusModel ?? '',
    sonnetModel: existing?.sonnetModel ?? '', haikuModel: existing?.haikuModel ?? '',
    environmentText: existing?.environmentText ?? '', key: '', authType: existing?.sso ? 'oidc' : 'apiKey',
    oidcIssuer: existing?.oidcIssuer ?? '', oidcClientId: existing?.oidcClientId ?? '', oidcScope: existing?.oidcScope ?? '',
    oidcTokenType: existing?.oidcTokenType ?? 'access', models: [], loading: false, saving: false,
  };
  const managed = existing?.managed ?? false;
  const dlg = openDialog(existing ? t('Bearbeiten') : t('Neues Profil'), (close) => {
    const host = endpointHost(s.endpoint);
    const allowed = !host || isHostAllowed(host);
    const invalid = invalidEnvironmentLines(s.environmentText);
    const sso = s.authType === 'oidc';
    const issuerOk = !sso || /^https:\/\//i.test(s.oidcIssuer.trim()) || /^http:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/i.test(s.oidcIssuer.trim());
    const valid = s.name.trim() && host && allowed && (sso ? issuerOk && s.oidcClientId.trim() : existing || s.key) && !invalid.length;
    const update = (key) => (value) => { s[key] = value; dlg.render(); };
    const modelField = (label, key, id) => field(label, input({ value: s[key], placeholder: t('Claude-Standard'), list: 'gateway-models', disabled: managed }, update(key)), id);
    const loadModels = async () => {
      s.loading = true; dlg.render();
      const models = await call('available_models', { endpoint: s.endpoint, typedKey: s.key, profileId: existing?.id ?? null });
      s.loading = false;
      if (models?.length) {
        s.models = models;
        // Fill empty tiers with the best match, keep what the user already chose.
        const suggestion = await call('suggest_models', { models });
        for (const tier of ['opus', 'sonnet', 'haiku']) if (!s[`${tier}Model`]) s[`${tier}Model`] = suggestion?.[tier] ?? '';
      }
      dlg.render();
    };
    const save = async () => {
      s.saving = true; dlg.render();
      const result = await call('save_profile', { input: { id: existing?.id ?? null, ...s } });
      s.saving = false;
      if (result?.[0]) close(); else dlg.render();
    };
    return {
      body: [
        h('h2', null, existing ? existing.name : t('Neues Profil')),
        h('fieldset', { disabled: managed },
          field(t('Name'), input({ value: s.name, placeholder: t('z. B. Projekt Alpha') }, update('name')), 'p-name'),
          field(t('LiteLLM-Endpunkt'), input({ value: s.endpoint, placeholder: 'https://litellm.firma.intern' }, update('endpoint')), 'p-endpoint'),
          s.endpoint && !host && h('div', { class: 'field-note error-text' }, t('Bitte eine vollständige https-URL angeben (http nur für localhost).')),
          host && !allowed && h('div', { class: 'field-note error-text' }, t('Host nicht freigegeben. Erlaubt: %@', view.allowedHosts.join(', ')))),
        managed && h('div', { class: 'section-note' }, sso ? t('Von der IT vorgegeben – nur die Anmeldung ist möglich.') : t('Von der IT vorgegeben – nur der Key kann geändert werden.')),
        h('fieldset', { disabled: !!existing },
          field(t('Anmeldung'), h('select', { onchange: (e) => { s.authType = e.target.value; dlg.render(); } },
            h('option', { value: 'apiKey', selected: !sso }, t('Key')),
            h('option', { value: 'oidc', selected: sso }, 'SSO')), 'p-auth')),
        sso ? h('fieldset', { disabled: managed },
          field(t('Issuer'), input({ value: s.oidcIssuer, placeholder: 'https://login.microsoftonline.com/<tenant>/v2.0' }, update('oidcIssuer')), 'p-issuer'),
          s.oidcIssuer.trim() && !issuerOk && h('div', { class: 'field-note error-text' }, t('Die SSO-Adresse muss https verwenden: %@', s.oidcIssuer.trim())),
          field(t('Client-ID'), input({ value: s.oidcClientId }, update('oidcClientId')), 'p-client'),
          field(t('Scope'), input({ value: s.oidcScope, placeholder: 'openid profile offline_access' }, update('oidcScope')), 'p-scope'),
          field(t('Token-Art'), h('select', { onchange: (e) => { s.oidcTokenType = e.target.value; } },
            h('option', { value: 'access', selected: s.oidcTokenType !== 'id' }, t('Access-Token')),
            h('option', { value: 'id', selected: s.oidcTokenType === 'id' }, t('ID-Token'))), 'p-token-type'),
          h('div', { class: 'section-note' }, t('Nach dem Anlegen im Profil „Anmelden“ wählen. Tokens liegen nur lokal in einer Datei, die nur dein Benutzerkonto lesen kann. Wird Issuer oder Client-ID geändert, wirst du abgemeldet.')),
          h('div', { class: 'section-note' }, t('SSO-Profile haben keinen Key und nehmen nicht am Budget-Killer-Pool teil.')))
        : h('fieldset', null,
          field(existing ? t('Neuer Key') : t('Key'), input({ type: 'password', value: s.key,
            placeholder: existing ? t('leer lassen, um den Key zu behalten') : 'sk-…' }, update('key')), 'p-key'),
          h('div', { class: 'section-note' }, t('Der Key wird nur lokal in einer Datei gespeichert, die nur dein Benutzerkonto lesen kann – nie im Projekt.'))),
        h('fieldset', null, h('legend', null, t('Modelle')),
          modelField('Opus', 'opusModel', 'p-opus'), modelField('Sonnet', 'sonnetModel', 'p-sonnet'),
          modelField('Haiku', 'haikuModel', 'p-haiku'), modelField(t('Standardmodell'), 'modelAlias', 'p-alias'),
          h('datalist', { id: 'gateway-models' }, s.models.map((m) => h('option', { value: m }))),
          h('div', { class: 'line' },
            button(t('Modelle vom Gateway laden'), loadModels,
              { disabled: managed || !host || !allowed || s.loading || (sso ? !existing : !existing && !s.key) }),
            s.loading && h('span', { class: 'muted' }, '…'),
            s.models.length > 0 && h('span', { class: 'muted small' }, t('%@ Modelle verfügbar', s.models.length))),
          h('div', { class: 'section-note' }, t('Eigene Modellnamen des Gateways für die Modellstufen von Claude Code. Leer lassen, um den Claude-Standard zu verwenden.'))),
        h('fieldset', { disabled: managed }, h('legend', null, t('Weitere Umgebungsvariablen')),
          h('textarea', { id: 'p-env', value: s.environmentText, spellcheck: 'false', 'aria-label': t('Weitere Umgebungsvariablen'),
            oninput: (e) => { s.environmentText = e.target.value; dlg.render(); } }),
          invalid.map((line) => h('div', { class: 'error-text' }, t('Ungültig oder nicht erlaubt: %@', line))),
          h('div', { class: 'section-note' }, t('Eine pro Zeile im Format NAME=Wert, z. B. CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 für Bedrock über LiteLLM. Key, Endpunkt und Modelle werden oben gesetzt.'))),
      ],
      actions: [button(t('Abbrechen'), close), button(existing ? t('Sichern') : t('Anlegen'), save, { cls: 'primary', disabled: !valid || s.saving })],
    };
  }, { wide: true });
  restoreCaret(dlg);
}

/** Keeps the caret at the end of the field being typed in when a dialog re-renders. */
function restoreCaret(dlg) {
  const render = dlg.render;
  dlg.render = () => {
    const active = document.activeElement;
    const position = active?.selectionStart;
    render();
    const now = document.activeElement;
    if (now && now.id === active?.id && position != null && 'setSelectionRange' in now) {
      try { now.setSelectionRange(position, position); } catch { /* not a text field */ }
    }
  };
}

function replaceKeyDialog(profile) {
  let key = '';
  const dlg = openDialog(t('Neuer Key'), (close) => ({
    body: [
      h('h2', null, profile.name),
      field(t('Neuer Key'), input({ type: 'password', value: key, placeholder: 'sk-…' }, (v) => { key = v; dlg.render(); }), 'k-key'),
      h('div', { class: 'section-note' }, t('Der bisherige Key für „%@“ wird ersetzt. Andere Profile bleiben unverändert.', profile.name)),
    ],
    actions: [button(t('Abbrechen'), close),
      button(t('Ersetzen'), async () => { close(); await call('store_key', { profileId: profile.id, key }); }, { cls: 'primary', disabled: !key.trim() })],
  }));
  restoreCaret(dlg);
  document.getElementById('k-key')?.focus();
}

function addProjectDialog(preselected) {
  const s = { picked: null, profileId: preselected ?? (view.profiles.length === 1 ? view.profiles[0].id : '') };
  const dlg = openDialog(t('Projekt zuordnen'), (close) => {
    const existing = s.picked?.existingProfileId;
    return {
      body: [
        h('h2', null, t('Projekt zuordnen')),
        h('div', { class: 'field' }, h('label', null, t('Projektordner')),
          h('div', { class: 'line' },
            h('span', { class: `spacer mono ${s.picked ? '' : 'muted'}` }, s.picked?.folder ?? t('Kein Ordner gewählt')),
            button(t('Auswählen …'), async () => { const picked = await call('pick_project_folder'); if (picked) { s.picked = picked; dlg.render(); } }))),
        s.picked && s.picked.folder !== s.picked.root && h('div', { class: 'note' }, icon('info', 'accent'),
          t('Der Ordner gehört zum Git-Repository %@. Claude Code liest Projekteinstellungen nur dort; die Zuordnung gilt für das ganze Repository inkl. Unterordnern und Worktrees.', s.picked.root)),
        field(t('Profil'), h('select', { onchange: (e) => { s.profileId = e.target.value; dlg.render(); } },
          h('option', { value: '', selected: !s.profileId }, t('Bitte wählen')),
          view.profiles.map((p) => h('option', { value: p.id, selected: p.id === s.profileId }, profileLabel(p)))), 'a-profile'),
        existing && h('div', { class: 'note' }, icon('rotate', 'accent'), t('Bereits Profil „%@“ zugeordnet – wird umgestellt.', profileName(existing))),
      ],
      actions: [button(t('Abbrechen'), close),
        button(t('Zuordnen'), async () => { close(); if (await ensureLogin(s.profileId)) call('bind', { folder: s.picked.folder, profileId: s.profileId }); },
          { cls: 'primary', disabled: !s.picked || !s.profileId })],
    };
  });
}

function exportDialog() {
  const s = { password: '', confirmation: '', working: false };
  const dlg = openDialog(t('Backup exportieren'), (close) => {
    const min = view.minimumPasswordLength;
    const valid = s.password.length >= min && s.password === s.confirmation;
    return {
      body: [
        h('h2', null, t('Backup exportieren')),
        field(t('Passwort'), input({ type: 'password', value: s.password }, (v) => { s.password = v; dlg.render(); }), 'e-password'),
        field(t('Passwort wiederholen'), input({ type: 'password', value: s.confirmation }, (v) => { s.confirmation = v; dlg.render(); }), 'e-confirm'),
        s.confirmation && s.password !== s.confirmation && h('div', { class: 'field-note error-text' }, t('Die Passwörter stimmen nicht überein.')),
        h('div', { class: 'section-note' }, t('Mindestens %@ Zeichen. Ohne dieses Passwort lässt sich das Backup nicht wiederherstellen – es wird nirgends gespeichert.', min)),
        h('div', { class: 'note' }, icon('fileLock', 'accent'), view.allowKeyExport
          ? t('Enthält Profile, Projektzuordnungen und Keys – verschlüsselt mit AES-256-GCM.')
          : t('Enthält Profile und Projektzuordnungen. Keys sind laut Firmenrichtlinie vom Export ausgenommen.')),
      ],
      actions: [button(t('Abbrechen'), close, { disabled: s.working }),
        button(s.working ? '…' : t('Exportieren …'), async () => {
          s.working = true; dlg.render();
          const ok = await call('export_backup', { password: s.password });
          s.working = false;
          if (ok) close(); else dlg.render();
        }, { cls: 'primary', disabled: !valid || s.working })],
    };
  });
  restoreCaret(dlg);
}

function importDialog() {
  const s = { path: null, password: '', working: false };
  const dlg = openDialog(t('Backup importieren'), (close) => ({
    body: [
      h('h2', null, t('Backup importieren')),
      h('div', { class: 'field' }, h('label', null, t('Backup-Datei')),
        h('div', { class: 'line' },
          h('span', { class: `spacer mono ${s.path ? '' : 'muted'}` }, s.path ?? t('Keine Datei gewählt')),
          button(t('Auswählen …'), async () => { const path = await call('pick_backup_file'); if (path) { s.path = path; dlg.render(); } }))),
      field(t('Passwort'), input({ type: 'password', value: s.password }, (v) => { s.password = v; dlg.render(); }), 'i-password'),
      h('div', { class: 'section-note' }, t('Profile und Keys aus dem Backup überschreiben vorhandene mit gleicher ID. Projekte werden zugeordnet, sofern ihr Ordner auf diesem Rechner existiert.')),
    ],
    actions: [button(t('Abbrechen'), close, { disabled: s.working }),
      button(s.working ? '…' : t('Importieren'), async () => {
        s.working = true; dlg.render();
        const ok = await call('import_backup', { path: s.path, password: s.password });
        s.working = false;
        if (ok) close(); else dlg.render();
      }, { cls: 'primary', disabled: !s.path || !s.password || s.working })],
  }));
  restoreCaret(dlg);
}

function claudeSettingsDialog() {
  let selected = view.globalProfileId ?? '';
  const dlg = openDialog(t('Claude-Einstellungen prüfen'), (close) => {
    const findings = view.globalFindings;
    const needsRepair = findings.some((f) => f.severity === 'error' || f.id.startsWith('plaintext-'));
    const health = view.globalHealth;
    const unchanged = (selected || null) === (view.globalProfileId ?? null) && health?.state === 'active';
    const healthText = !health ? null : {
      active: t('Standardprofil ist aktiv.'),
      notApplied: t('Standardprofil fehlt in der Datei – „Übernehmen“ wählen.'),
      drifted: t('Abweichung in %@ – „Übernehmen“ wählen.', health.keys.join(', ')),
      folderMissing: t('Datei nicht gefunden.'),
    }[health.state];
    const severityIcon = { error: ['error', 'red'], warning: ['warning', 'orange'], info: ['info', 'muted'] };
    return {
      body: [
        h('h2', null, t('Claude-Einstellungen prüfen')),
        h('fieldset', null, h('legend', { class: 'mono' }, view.userSettingsPath),
          findings.every((f) => f.severity === 'info') && h('div', { class: 'finding green' }, icon('check'), t('Keine Probleme gefunden.')),
          findings.map((f) => h('div', { class: `finding ${severityIcon[f.severity][1]}` }, icon(severityIcon[f.severity][0]), h('span', { class: 'selectable' }, f.message))),
          h('div', { class: 'line' },
            button(t('Erneut prüfen'), () => call('refresh_all')),
            button(view.userSettingsExists ? t('Reparieren') : t('Erzeugen'), () => call('repair_user_settings'),
              { disabled: view.userSettingsExists && !needsRepair }),
            h('span', { class: 'spacer' }),
            button(revealLabel(), () => call('reveal', { path: view.userSettingsFullPath }), { iconName: 'folder', disabled: !view.userSettingsExists })),
          h('div', { class: 'section-note' }, t('„Reparieren“ macht die Datei gültig, entfernt Klartext-Keys und wandelt Werte in „env“ in Text um. Vorher legt KeyZapper eine Sicherung settings.json.keyzapper-<Zeit>.bak an.'))),
        h('fieldset', null, h('legend', null, t('Standardprofil für alle übrigen Ordner')),
          field(t('Standardprofil'), h('select', { onchange: (e) => { selected = e.target.value; dlg.render(); } },
            h('option', { value: '', selected: !selected }, t('Keins')),
            view.profiles.map((p) => h('option', { value: p.id, selected: p.id === selected }, profileLabel(p)))), 'c-profile'),
          healthText && h('div', { class: `finding ${health.state === 'active' ? 'green' : 'orange'}` },
            icon(health.state === 'active' ? 'check' : 'warning'), healthText),
          h('div', { class: 'line' }, h('span', { class: 'spacer' }),
            button(t('Übernehmen'), async () => { if (!selected || await ensureLogin(selected)) call('set_global_profile', { profileId: selected || null }); },
              { disabled: unchanged || (!selected && !view.globalProfileId) || view.disabled })),
          h('div', { class: 'section-note' }, t('Trägt das Profil in ~/.claude/settings.json ein. Claude Code nutzt es in allen Ordnern ohne eigene Zuordnung, statt auf einen Klartext-Key oder die normale Anmeldung zurückzufallen. Zugeordnete Projekte behalten ihr eigenes Profil.'))),
      ],
      actions: [button(t('Fertig'), close, { cls: 'primary' })],
    };
  }, { live: true, wide: true });
}

const agentGoals = () => [
  ['cost', t('Kosteneffizienz'), t('Wenig Tokens und Tool-Aufrufe, kleine Diffs, günstigstes geeignetes Modell')],
  ['quality', t('Qualität & Korrektheit'), t('Gründlich prüfen, Tests und Builds, Diff vor Abschluss reviewen')],
  ['speed', t('Tempo'), t('Kurze Antworten, wenig Rückfragen, unabhängige Schritte parallel')],
  ['security', t('Sicherheit'), t('Keine Secrets, keine destruktiven Befehle ohne Rückfrage, Abhängigkeiten prüfen')],
  ['conventions', t('Team-Konventionen'), t('Stil, Commits, Branches und Pull Requests wie im Repository')],
];

/** Scans the project, lets the gateway model assess it and propose an AGENTS.md, which is saved after review. */
function agentsDialog(binding, profile) {
  const tiers = [['Opus', profile.opusModel], ['Sonnet', profile.sonnetModel], ['Haiku', profile.haikuModel], [t('Standardmodell'), profile.modelAlias]]
    .filter(([, model], i, all) => model && all.findIndex(([, m]) => m === model) === i);
  const s = { scan: null, scanError: null, goals: new Set(['cost']), language: view.language, model: tiers[0]?.[1] ?? '', notes: '',
    working: false, proposal: null, content: '' };
  const generate = async () => {
    if (!(await ensureLogin(profile.id))) return;
    s.working = true; dlg.render();
    const proposal = await call('generate_agents_file', { bindingId: binding.id,
      options: { goals: [...s.goals], language: s.language, model: s.model, notes: s.notes } });
    s.working = false;
    if (proposal) { s.proposal = proposal; s.content = proposal.content; }
    dlg.render();
  };
  const save = async (close) => {
    if (s.scan?.hasAgentsFile && !(await confirmDialog(t('AGENTS.md ersetzen?'),
      t('Die vorhandene AGENTS.md in „%@“ wird überschrieben.', binding.name), t('Ersetzen')))) return;
    const result = await call('write_agents_file', { bindingId: binding.id, content: s.content });
    if (result?.[0]) { close(); window.scrollTo(0, 0); }
  };
  const scanSummary = () => {
    if (s.scanError) return h('div', { class: 'error-text' }, s.scanError);
    if (!s.scan) return h('div', { class: 'line muted' }, h('span', { class: 'spinner' }), t('Repository wird gelesen …'));
    const languages = s.scan.languages.slice(0, 6).map(([name, count]) => `${name} ${count}`).join(' · ');
    return [
      h('div', null, h('strong', null, t('%@ Dateien', s.scan.fileCount)), languages && h('span', { class: 'muted' }, ` · ${languages}`)),
      s.scan.manifests.length > 0 && h('div', { class: 'mono muted' }, s.scan.manifests.join(', ')),
      h('div', { class: 'finding' }, icon(s.scan.agentFiles.length ? 'check' : 'info', s.scan.agentFiles.length ? 'green' : 'muted'),
        s.scan.agentFiles.length ? t('Vorhandene Agenten-Dateien: %@', s.scan.agentFiles.join(', ')) : t('Noch keine Agenten-Steuerdatei vorhanden.')),
    ];
  };
  const options = () => [
    h('fieldset', null, h('legend', null, t('Repository')), scanSummary()),
    h('fieldset', { disabled: s.working }, h('legend', null, t('Optimieren für')),
      agentGoals().map(([id, title, detail]) => h('label', { class: 'goal' },
        h('input', { type: 'checkbox', checked: s.goals.has(id), onchange: (e) => { if (e.target.checked) s.goals.add(id); else s.goals.delete(id); dlg.render(); } }),
        h('span', null, h('strong', null, title), h('span', { class: 'muted' }, ` – ${detail}`))))),
    h('fieldset', { disabled: s.working },
      field(t('Sprache der Datei'), h('select', { onchange: (e) => { s.language = e.target.value; } },
        h('option', { value: 'de', selected: s.language === 'de' }, 'Deutsch'),
        h('option', { value: 'en', selected: s.language !== 'de' }, 'English')), 'g-language'),
      field(t('Analysemodell'), h('select', { onchange: (e) => { s.model = e.target.value; } },
        tiers.map(([tier, model]) => h('option', { value: model, selected: s.model === model }, `${tier} · ${model}`)),
        h('option', { value: '', selected: !s.model }, t('Automatisch (vom Gateway)'))), 'g-model'),
      h('div', { class: 'field' }, h('label', { for: 'g-notes' }, t('Hinweise')),
        h('textarea', { id: 'g-notes', value: s.notes, placeholder: t('optional, z. B. „Nur pnpm verwenden“ oder „Migrationen nie selbst ausführen“'),
          oninput: (e) => { s.notes = e.target.value; } }))),
    !profile.sso && !profile.hasKey && h('div', { class: 'note red' }, icon('keyOff'), t('Kein Key hinterlegt – Anfragen schlagen fehl')),
    s.working ? h('div', { class: 'line muted', role: 'status' }, h('span', { class: 'spinner' }), t('Das Modell analysiert das Repository – das kann ein bis zwei Minuten dauern …'))
      : h('div', { class: 'section-note' }, t('Gesendet wird eine Zusammenfassung des Repositorys (Ordnerstruktur, README, Build- und CI-Dateien, vorhandene Agenten-Dateien) über das Gateway von „%@“. Die Kosten werden auf diesen Key gebucht; geschrieben wird erst nach deiner Prüfung.', profile.name)),
  ];
  const review = () => {
    const p = s.proposal;
    return [
      h('fieldset', null, h('legend', null, t('Bewertung')),
        h('div', { class: 'selectable' }, p.assessment || t('Keine Bewertung geliefert.')),
        h('div', { class: 'small muted' }, t('Analysiert mit %@ · %@ Eingabe- und %@ Ausgabe-Tokens', p.model,
          p.inputTokens.toLocaleString(locale()), p.outputTokens.toLocaleString(locale())))),
      p.recommendations.length > 0 && h('fieldset', null, h('legend', null, t('Modellempfehlung')),
        h('div', { class: 'recommendations' }, p.recommendations.map((r) => [
          h('strong', null, r.task),
          h('span', { class: 'mono' }, r.model, ['opus', 'sonnet', 'haiku'].includes(r.tier) && !r.model.toLowerCase().includes(r.tier) ? ` (${r.tier})` : ''),
          h('span', { class: 'muted selectable' }, r.reason)]))),
      h('fieldset', null, h('legend', null, 'AGENTS.md'),
        h('textarea', { id: 'g-content', class: 'document', value: s.content, spellcheck: 'false', 'aria-label': 'AGENTS.md',
          oninput: (e) => { s.content = e.target.value; } }),
        s.scan?.hasAgentsFile && h('div', { class: 'note orange' }, icon('warning'), t('Die vorhandene AGENTS.md wird ersetzt; ihr Inhalt ist in den Vorschlag eingeflossen.')),
        s.scan?.agentFiles.some((f) => f !== 'AGENTS.md') && h('div', { class: 'section-note' },
          t('Andere Agenten-Dateien (%@) bleiben unverändert. Doppelte Anweisungen dort entfernen, sonst lädt der Agent sie zweimal.', s.scan.agentFiles.filter((f) => f !== 'AGENTS.md').join(', ')))),
    ];
  };
  const dlg = openDialog(t('Für Agenten optimieren'), (close) => ({
    body: [h('h2', null, t('Für Agenten optimieren'), h('span', { class: 'muted' }, ` · ${binding.name}`)), s.proposal ? review() : options()],
    actions: s.proposal
      ? [button(t('Zurück'), () => { s.proposal = null; dlg.render(); }, { cls: 'left' }), button(t('Abbrechen'), close),
        button(s.scan?.hasAgentsFile ? t('AGENTS.md ersetzen') : t('AGENTS.md speichern'), () => save(close), { cls: 'primary', disabled: !s.content.trim() })]
      : [button(t('Abbrechen'), close, { disabled: s.working }),
        button(t('Analysieren & erzeugen'), generate, { cls: 'primary', disabled: !s.scan || s.working || (!profile.sso && !profile.hasKey) })],
  }), { wide: true });
  invoke('scan_repository', { bindingId: binding.id })
    .then((scan) => { s.scan = scan; }, (error) => { s.scanError = String(error); })
    .finally(() => dlg.render());
}

/** Once after an update: the highlights of the releases since the version last seen. */
function whatsNewDialog() {
  whatsNewShown = true;
  const releases = view.whatsNew;
  const { dialog } = openDialog(t('Neu in KeyZapper %@', view.appVersion), (close) => ({
    body: [
      h('h2', null, t('Neu in KeyZapper %@', view.appVersion)),
      releases.map((r) => h('fieldset', null, releases.length > 1 && h('legend', null, `KeyZapper ${r.version}`),
        h('ul', { class: 'whats-new' }, r.items.map((item) => h('li', null, item))))),
    ],
    actions: [
      button(t('Versionshinweise'), () => call('open_url', { url: `${view.projectUrl}/releases/tag/${view.appVersion}` }), { cls: 'left' }),
      button(t('Weiter'), close, { cls: 'primary' }),
    ],
  }), { wide: true });
  dialog.addEventListener('close', () => call('dismiss_whats_new'));
}

function howItWorksDialog() {
  const node = (symbol, title, subtitle) => h('div', { class: 'flow-node' }, icon(symbol), h('strong', null, title), h('span', { class: 'mono' }, subtitle));
  const arrow = () => icon('arrow', 'muted');
  const step = (n, title, text) => h('div', { class: 'step' }, h('span', { class: 'step-number' }, n),
    h('div', null, h('strong', null, title), h('div', { class: 'muted' }, text)));
  const hint = (symbol, text) => h('div', { class: 'finding' }, icon(symbol, 'muted'), text);
  openDialog(t('So funktioniert KeyZapper'), (close) => ({
    body: [
      h('h2', null, t('So funktioniert KeyZapper')),
      h('strong', { class: 'green' }, t('Einmal zuordnen – danach musst du nie wieder Keys wechseln.')),
      h('div', { class: 'flow' },
        node('terminal', 'Claude Code', t('im Projekt')), arrow(),
        node('bolt', 'keyzapper-helper', '--profile'), arrow(),
        node('fileLock', t('Key-Datei'), t('Projekt-Key')), arrow(),
        node('server', 'LiteLLM', t('→ Bedrock'))),
      step(1, t('Einmal einrichten'), t('Profil mit Key anlegen und Projektordner zuordnen. Der Key liegt nur lokal in deinem Benutzerkonto, nie im Projekt.')),
      step(2, t('Verweis statt Key'), t('KeyZapper schreibt in den Projektordner .claude/settings.local.json – darin steht nur, welcher Helper und welches Profil gilt, nie der Key selbst.')),
      step(3, t('Claude holt den Key selbst'), t('Startet Claude Code in VS Code, IntelliJ oder im Terminal, ruft es den Helper auf. Der liefert genau den Key dieses Projekts – auch wenn KeyZapper geschlossen ist.')),
      step(4, t('Mehrere Projekte parallel'), t('Jedes Projekt nutzt seinen eigenen Key, gleichzeitig und unabhängig voneinander.')),
      h('fieldset', null, h('legend', null, t('Gut zu wissen')),
        hint('rotate', t('Nach dem Zuordnen oder einem Profilwechsel die Claude-Sitzung im Projekt einmal neu starten.')),
        hint('key', t('Neuer Key? Einmal „Ändern“ – alle zugeordneten Projekte nutzen ihn automatisch. Neue Sitzungen sofort, laufende nach spätestens 5 Minuten.')),
        hint('folder', t('Die Zuordnung gilt für das ganze Repository samt Unterordnern und Worktrees. Außerhalb zugeordneter Ordner nutzt Claude die normale Anmeldung.')),
        hint('shield', t('Fehlt ein Key, schlagen Anfragen fehl. Es wird nie ein anderer Key verwendet.'))),
    ],
    actions: [button(t('Verstanden'), close, { cls: 'primary' })],
  }), { wide: true });
}

// MARK: Tooltips

/** Shows `title` texts as a quick tooltip; the WebViews show native ones late or not at all. A popover, so it is above open dialogs. */
const tooltip = h('div', { class: 'tooltip', role: 'tooltip', popover: 'manual' });
let tipTarget = null;
let tipTimer = null;

function hideTip() {
  clearTimeout(tipTimer);
  tipTarget = null;
  if (tooltip.matches(':popover-open')) tooltip.hidePopover();
}

function showTip(target) {
  if (!target.isConnected) return;
  tooltip.textContent = target.dataset.tip;
  if (!tooltip.matches(':popover-open')) tooltip.showPopover();
  const rect = target.getBoundingClientRect();
  const { offsetWidth: width, offsetHeight: height } = tooltip;
  const below = rect.bottom + 6 + height <= window.innerHeight;
  tooltip.style.top = `${below ? rect.bottom + 6 : rect.top - 6 - height}px`;
  tooltip.style.left = `${Math.max(8, Math.min(rect.left + rect.width / 2 - width / 2, window.innerWidth - width - 8))}px`;
}

document.addEventListener('mouseover', (event) => {
  const target = event.target.closest?.('[title], [data-tip]');
  if (target === tipTarget) return;
  hideTip();
  if (!target) return;
  // Moved to data-tip so the native tooltip does not show as well.
  if (target.hasAttribute('title')) {
    target.dataset.tip = target.title;
    if (target.getAttribute('aria-label') !== target.title) target.setAttribute('aria-description', target.title);
    target.removeAttribute('title');
  }
  if (!target.dataset.tip) return;
  tipTarget = target;
  tipTimer = setTimeout(() => showTip(target), 350);
});
for (const type of ['mousedown', 'keydown', 'scroll']) document.addEventListener(type, hideTip, true);

// MARK: Start

async function start() {
  document.body.append(tooltip);
  english = await invoke('translations');
  setView(await invoke('get_view'));
  await listen('changed', async () => setView(await invoke('get_view')));
}

start();
