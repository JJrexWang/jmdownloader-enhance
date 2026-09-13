// =======================================================================
//  JMComic Downloader · WebUI
//  纯 vanilla JS,零依赖, 直接 fetch server 的 REST API + SSE 收事件。
// =======================================================================

// ----------- API 客户端 -----------
const API = {
  base: window.location.origin,
  async get(path) {
    const r = await fetch(this.base + path);
    if (!r.ok) throw await toApiError(r);
    return r.json();
  },
  async post(path, body) {
    const r = await fetch(this.base + path, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!r.ok) throw await toApiError(r);
    return r.json();
  },
};

async function toApiError(r) {
  let msg = `HTTP ${r.status}`;
  try {
    const j = await r.json();
    if (j.message) msg += `: ${j.message}`;
    if (j.err_title) msg = `[${j.err_title}] ${msg}`;
  } catch {}
  return new Error(msg);
}

// ----------- 全局状态 -----------
const State = {
  user: null,                 // { username, photo, ... }
  config: null,               // 后端 Config
  currentTab: 'search',
  detail: {                   // 右侧详情面板
    comic: null,              // 当前显示的 Comic
    chapters: [],             // 章节列表
  },
  // 进度面板: taskKey -> { title, downloaded, total, state, ... }
  tasks: new Map(),
  // 收藏夹 folders
  fav: { folders: [], active: null, comics: [], page: 1, sort: 'mr' },
  // 每周必看
  weekly: { info: null, category: null, type: null, comics: [] },
  // 搜索
  search: { keyword: '', page: 1, sort: 'mr', results: [], total: 0 },
  // 本地库存
  downloaded: { list: [] },
};

// taskKey 用于合并同一章节的多次 update
const taskKey = (comicId, chapterId) => `${comicId}::${chapterId}`;

// ----------- DOM 工具 -----------
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === 'class') e.className = v;
    else if (k === 'onclick') e.addEventListener('click', v);
    else if (k === 'onchange') e.addEventListener('change', v);
    else if (k === 'oninput') e.addEventListener('input', v);
    else if (k === 'html') e.innerHTML = v;
    else if (v !== undefined && v !== null) e.setAttribute(k, v);
  }
  function appendAll(node, items) {
    for (const c of items) {
      if (c === null || c === undefined || c === false) continue;
      if (Array.isArray(c)) appendAll(node, c);
      else node.appendChild(typeof c === 'string' || typeof c === 'number'
        ? document.createTextNode(String(c)) : c);
    }
  }
  appendAll(e, children);
  return e;
}
function escapeHtml(s) {
  return String(s ?? '').replace(/[&<>"']/g, c => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}
function toast(msg, type = 'info', ms = 3000) {
  const t = el('div', { class: `toast ${type}` }, msg);
  $('#toast-host').appendChild(t);
  setTimeout(() => t.remove(), ms);
}

// ----------- tab 切换 -----------
$$('.tab').forEach(btn => {
  btn.addEventListener('click', () => switchTab(btn.dataset.tab));
});
function switchTab(name) {
  State.currentTab = name;
  $$('.tab').forEach(b => b.classList.toggle('active', b.dataset.tab === name));
  $$('.pane').forEach(p => p.classList.toggle('active', p.dataset.pane === name));
  // 懒加载
  if (name === 'config' && !State.config) loadConfig();
  if (name === 'downloaded' && State.downloaded.list.length === 0) loadDownloaded();
  if (name === 'favorite' && State.fav.folders.length === 0) loadFavorite();
  if (name === 'weekly' && !State.weekly.info) loadWeekly();
}

// ----------- 搜索 -----------
$('#search-btn').addEventListener('click', () => doSearch(1));
$('#search-input').addEventListener('keydown', e => {
  if (e.key === 'Enter') doSearch(1);
});
$('#search-sort').addEventListener('change', e => {
  State.search.sort = e.target.value;
  if (State.search.keyword) doSearch(1);
});
$('#search-pager').addEventListener('click', e => {
  const btn = e.target.closest('button[data-page]');
  if (!btn) return;
  doSearch(btn.dataset.page === 'next' ? State.search.page + 1 : State.search.page - 1);
});
async function doSearch(page) {
  const keyword = $('#search-input').value.trim();
  if (!keyword) { toast('请输入关键词', 'warning'); return; }
  State.search.keyword = keyword;
  State.search.page = page;
  try {
    const data = await API.post('/search', {
      keyword, page, sort: State.search.sort,
    });
    // /search 返回 SearchResultVariant:
    //   { SearchResult: { searchQuery, total, content: [...] } }
    //   | { Comic: <单本> }
    let list = [];
    if (Array.isArray(data)) list = data;
    else if (data?.SearchResult?.content) list = data.SearchResult.content;
    else if (data?.searchResult?.content) list = data.searchResult.content;
    else if (data?.Comic) list = [data.Comic];
    else if (data?.comic) list = [data.comic];
    State.search.results = list;
    State.search.total = data?.SearchResult?.total ?? data?.searchResult?.total ?? list.length;
    renderSearch();
  } catch (err) { toast('搜索失败: ' + err.message, 'error'); }
}
function renderSearch() {
  const host = $('#search-results');
  host.innerHTML = '';
  if (State.search.results.length === 0) {
    host.appendChild(el('p', { class: 'muted' }, '无结果'));
    $('#search-pager').classList.add('hidden');
    return;
  }
  for (const c of State.search.results) {
    host.appendChild(renderComicCard(c, c.id ?? c.comicId));
  }
  $('#search-pager').classList.remove('hidden');
  $('#search-page-info').textContent = `第 ${State.search.page} 页`;
}

// ----------- 漫画卡片 (通用 search/favorite/weekly/downloaded) -----------
function renderComicCard(c, comicId) {
  const title = c.name ?? c.title ?? '(无标题)';
  const author = c.author ?? '';
  const cover = buildCoverUrl(c, comicId);
  const isDownloaded = c.is_downloaded ?? c.isDownloaded;
  const isDownloading = c.is_downloading ?? c.isDownloading;
  return el('div', { class: 'card' },
    el('div', { class: 'cover' },
      cover ? el('img', { src: cover, loading: 'lazy', referrerpolicy: 'no-referrer',
                          onerror: "this.style.display='none'" })
            : el('span', { class: 'muted' }, '📖')
    ),
    el('div', { class: 'body' },
      el('div', { class: 'title' }, title),
      el('div', { class: 'meta' },
        author ? [el('span', {}, author), ' · '] : [],
        isDownloaded ? el('span', { class: 'badge downloaded' }, '已下') : null,
        isDownloading ? el('span', { class: 'badge downloading' }, '下载中') : null,
      ),
    ),
  );
}
// 跟桌面 Tauri 版 ComicCard.vue 同源: JM API 现在 search/weekly/favorite 返回的
// `image` 字段经常是空串,但封面 URL 实际上可以用 `comicId` + `_3x4.jpg` 拼出来。
// 走 no-referrer 直连,实测 cdn-msp3.18comic.vip 不强制 Referer。
function buildCoverUrl(c, comicId) {
  const fromApi = c.image ?? c.img_url ?? c.imgUrl ?? c.cover ?? c.thumbnail ?? '';
  if (fromApi) return fromApi;
  const id = comicId ?? c.id ?? c.comicId;
  if (!id) return '';
  return `https://cdn-msp3.18comic.vip/media/albums/${id}_3x4.jpg`;
}

// 重新包装: 给 card 整体加 onclick
const _origRender = renderComicCard;
function renderComicCardWithClick(c, comicId) {
  const card = _origRender(c, comicId);
  card.addEventListener('click', () => openComic(comicId));
  return card;
}
renderComicCard = renderComicCardWithClick;

// ----------- 打开漫画详情 -----------
async function openComic(comicId) {
  try {
    let comic = await API.get('/comic/' + comicId);
    // 跟本地索引合并
    try {
      comic = await API.post('/sync-comic', comic);
    } catch (e) { console.warn('sync-comic 失败', e); }
    State.detail.comic = comic;
    State.detail.chapters = (comic.chapter_infos || comic.chapterInfos || comic.chapters || []);
    renderDetail();
  } catch (err) { toast('加载漫画失败: ' + err.message, 'error'); }
}
function renderDetail() {
  const c = State.detail.comic;
  if (!c) return;
  $('#detail-empty').classList.add('hidden');
  $('#detail-content').classList.remove('hidden');
  // 兼容多种字段名:SearchResult 是 image,/comic/:id 是 img_url/cover 等历史命名
  $('#detail-cover').src = buildCoverUrl(c, c.id);
  $('#detail-title').textContent = c.name || c.title || '';
  // author 可能是字符串(搜索结果)或数组(/comic/:id 返回 Vec<String>),都要 flatten
  // pages 字段 Comic 不返回,只在 chapter 维度
  const authorStr = Array.isArray(c.author) ? c.author.filter(Boolean).join(', ') : (c.author || '');
  const pagesStr = c.pages ? c.pages + ' 页' : '';
  $('#detail-author').textContent = [authorStr, pagesStr].filter(Boolean).join(' · ');
  const tagsHost = $('#detail-tags');
  tagsHost.innerHTML = '';
  for (const t of (c.tags || [])) tagsHost.appendChild(el('span', { class: 'tag' }, t));

  const list = $('#chapter-list');
  list.innerHTML = '';
  let downloaded = 0;
  for (const ch of State.detail.chapters) {
    const chId = ch.chapter_id ?? ch.chapterId ?? ch.id;
    const title = ch.chapter_title ?? ch.chapterTitle ?? ch.title ?? `#${chId}`;
    const isDl = ch.is_downloaded ?? ch.isDownloaded;
    if (isDl) downloaded++;
    const li = el('li', {},
      el('input', { type: 'checkbox', class: 'ch-check', 'data-ch-id': chId }),
      el('span', { class: 'ch-title' }, title),
      el('span', { class: 'ch-status' },
        isDl ? '✓ 已下载' : (ch.download_status || '')),
    );
    list.appendChild(li);
  }
  $('#ch-count').textContent = `${downloaded} / ${State.detail.chapters.length} 已下载`;
}
$('#detail-close').addEventListener('click', () => {
  $('#detail-empty').classList.remove('hidden');
  $('#detail-content').classList.add('hidden');
  State.detail.comic = null;
  State.detail.chapters = [];
});
$('#ch-select-all').addEventListener('change', e => {
  $$('.ch-check').forEach(c => c.checked = e.target.checked);
});

// ----------- 章节下载 / 导出 -----------
async function withCheckedChapters(fn) {
  const ids = $$('.ch-check:checked').map(c => Number(c.dataset.chId));
  if (ids.length === 0) { toast('请先勾选章节', 'warning'); return; }
  try { await fn(ids); toast('已提交', 'success'); }
  catch (err) { toast('失败: ' + err.message, 'error'); }
}
$('#detail-download-all').addEventListener('click', () => {
  const c = State.detail.comic;
  if (!c) return;
  const ids = State.detail.chapters.map(ch => Number(ch.chapter_id ?? ch.chapterId ?? ch.id));
  API.post('/download/tasks', { comic: c, chapter_ids: ids })
    .then(() => toast(`已加入 ${ids.length} 个下载任务`, 'success'))
    .catch(err => toast('失败: ' + err.message, 'error'));
});
$('#ch-download-selected').addEventListener('click', () => withCheckedChapters(ids => {
  return API.post('/download/tasks', {
    comic: State.detail.comic, chapter_ids: ids,
  });
}));
$('#detail-export-cbz').addEventListener('click', () => {
  if (!State.detail.comic) return;
  API.post('/export/cbz', State.detail.comic)
    .then(() => toast('已加入 CBZ 导出队列', 'success'))
    .catch(err => toast('失败: ' + err.message, 'error'));
});
$('#detail-export-pdf').addEventListener('click', () => {
  if (!State.detail.comic) return;
  API.post('/export/pdf', State.detail.comic)
    .then(() => toast('已加入 PDF 导出队列', 'success'))
    .catch(err => toast('失败: ' + err.message, 'error'));
});
$('#ch-export-cbz').addEventListener('click', () => withCheckedChapters(ids => {
  return API.post('/export/cbz/chapters', { comic: State.detail.comic, chapter_ids: ids });
}));
$('#ch-export-pdf').addEventListener('click', () => withCheckedChapters(ids => {
  return API.post('/export/pdf/chapters', { comic: State.detail.comic, chapter_ids: ids });
}));

// ----------- 收藏夹 -----------
// 多重 fallback: 不同 (folder_id, sort) 组合调 /favorites,取第一个拿到非空 folderList 的结果。
// JM API 历史上 folder_id=0 / -1 / 省略、sort 用 'mr' / 'FavoriteTime' / 'mp' 行为都不一致,
// 一次失败就放弃太脆。Diagnostic 信息会打到 console + toast 里,方便定位。
async function loadFavorite() {
  const ATTEMPTS = [
    { folder_id: 0,  page: 1, sort: 'FavoriteTime' },  // 桌面 Tauri 默认
    { folder_id: 0,  page: 1, sort: 'mr' },            // mobile shorthand
    { folder_id: 0,  page: 1, sort: 'UpdateTime' },     // PascalCase 别名
    { folder_id: 0,  page: 1, sort: 'mp' },            // mobile 别名
    { folder_id: -1, page: 1, sort: 'mr' },            // 老 webui 写法 (兜底)
  ];
  const pickedFrom = [];
  let info = null;
  for (const body of ATTEMPTS) {
    try {
      const resp = await API.post('/favorites', body);
      const fl = resp?.folderList ?? resp?.folder_list ?? resp?.Folders ?? resp?.folders ?? [];
      console.log('[loadFavorite] try', body, '-> keys:', Object.keys(resp || {}), 'folderList.len:', fl.length);
      if (Array.isArray(fl) && fl.length > 0) {
        info = resp;
        pickedFrom.push(`${body.folder_id}/${body.sort} -> ${fl.length}`);
        break;
      }
    } catch (err) {
      console.log('[loadFavorite] try', body, 'failed:', err.message);
    }
  }
  if (!info) {
    // 最后一次尝试拿空响应,只为给 toast 提供诊断
    try { info = await API.post('/favorites', ATTEMPTS[0]); } catch {}
  }
  State.fav.folders = info?.folderList ?? info?.folder_list ?? info?.Folders ?? info?.folders ?? [];
  if (State.fav.folders.length === 0 && info) {
    // 也试 user-profile 兜底
    try {
      const profile = await API.get('/user-profile');
      const pfs = profile?.favorite_folders || profile?.data?.favorite_folders || [];
      if (Array.isArray(pfs) && pfs.length > 0) State.fav.folders = pfs;
    } catch {}
  }
  renderFavFolders();
  if (State.fav.folders.length > 0) {
    const first = State.fav.folders[0];
    const fid = first.FID ?? first.fid ?? first.id ?? first.ID;
    console.log('[loadFavorite] picked', pickedFrom, 'first fid:', fid, 'first:', first);
    selectFavFolder(fid);
  } else {
    // 诊断信息: 让用户立刻看到后端返回了什么
    const keys = info ? Object.keys(info).join(',') : 'null';
    const raw = info ? JSON.stringify(info).slice(0, 240) : 'no response';
    console.warn('[loadFavorite] 全部尝试都拿不到 folder_list, 后端响应 keys =', keys, 'raw =', raw);
    toast('收藏夹为空 (后端响应 keys=' + keys + ', raw=' + raw + ')', 'warning', 6000);
  }
}
function renderFavFolders() {
  const host = $('#fav-folders');
  host.innerHTML = '';
  for (const f of State.fav.folders) {
    // server 端 FavoriteFolderRespData 用 #[serde(rename = "FID")],
    // 所以 JSON 里是 FID;同时也兼容老代码里误用的 id/ID。
    const id = f.FID ?? f.fid ?? f.id ?? f.ID;
    const name = f.name ?? f.NAME ?? f.NAME_ ?? `folder ${id}`;
    const chip = el('div', {
      class: 'folder-chip' + (id === State.fav.active ? ' active' : ''),
      'data-folder-id': id,
    }, name);
    chip.addEventListener('click', () => selectFavFolder(id));
    host.appendChild(chip);
  }
}
async function selectFavFolder(id) {
  State.fav.active = id;
  renderFavFolders();
  try {
    const data = await API.post('/favorites', {
      folder_id: id, page: 1, sort: 'mr',
    });
    const list = data?.list || data?.List || data?.comics || (Array.isArray(data) ? data : []);
    State.fav.comics = list;
    const host = $('#fav-results');
    host.innerHTML = '';
    for (const c of list) {
      const id = c.id ?? c.comicId;
      host.appendChild(renderComicCard(c, id));
    }
  } catch (err) { toast('加载收藏列表失败: ' + err.message, 'error'); }
}
$('#fav-sync').addEventListener('click', () => {
  API.post('/sync-favorite').then(() => { toast('同步请求已发', 'success'); loadFavorite(); })
    .catch(err => toast('同步失败: ' + err.message, 'error'));
});
$('#fav-download-all').addEventListener('click', () => {
  if (!confirm('确认下载全部收藏夹?可能很多!')) return;
  API.post('/download/all-favorites').then(() => toast('已提交', 'success'))
    .catch(err => toast('失败: ' + err.message, 'error'));
});

// ----------- 每周必看 -----------
async function loadWeekly() {
  try {
    const info = await API.get('/weekly-info');
    State.weekly.info = info;
    const cats = info?.categories || info?.Categories || info?.data?.categories || [];
    const catSel = $('#weekly-category');
    catSel.innerHTML = '';
    for (const c of cats) {
      const id = c.id ?? c.ID ?? c.category_id;
      const name = c.title ?? c.name ?? c.NAME ?? `cat ${id}`;
      catSel.appendChild(el('option', { value: id }, name));
    }
    if (cats.length > 0) {
      State.weekly.category = cats[0].id ?? cats[0].ID;
      renderWeeklyTypes();
    }
    catSel.addEventListener('change', () => {
      State.weekly.category = catSel.value;
      renderWeeklyTypes();
    });
  } catch (err) { toast('加载每周必看失败: ' + err.message, 'error'); }
}
function renderWeeklyTypes() {
  const sel = $('#weekly-type');
  sel.innerHTML = '';
  // weekly-info 返回 { categories: [...], type: [{id, title}, ...] }
  // type 是顶层数组, 不在每个 category 下
  const types = State.weekly.info?.type || State.weekly.info?.Type || [];
  for (const t of types) {
    const id = t.id ?? t.ID;
    const name = t.title ?? t.name ?? t.NAME ?? `type ${id}`;
    sel.appendChild(el('option', { value: id }, name));
  }
  if (types.length > 0) {
    State.weekly.type = types[0].id ?? types[0].ID;
    loadWeeklyList();
  }
  sel.onchange = () => { State.weekly.type = sel.value; loadWeeklyList(); };
}
async function loadWeeklyList() {
  try {
    const data = await API.post('/weekly', {
      category_id: String(State.weekly.category),
      type_id: String(State.weekly.type),
    });
    const list = data?.list || data?.List || data?.comics || (Array.isArray(data) ? data : []);
    State.weekly.comics = list;
    const host = $('#weekly-results');
    host.innerHTML = '';
    for (const c of list) {
      host.appendChild(renderComicCard(c, c.id ?? c.comicId));
    }
  } catch (err) { toast('加载每周列表失败: ' + err.message, 'error'); }
}

// ----------- 本地库存 -----------
async function loadDownloaded() {
  try {
    const list = await API.get('/downloaded-comics');
    State.downloaded.list = list;
    $('#dl-count').textContent = `共 ${list.length} 本`;
    const host = $('#downloaded-results');
    host.innerHTML = '';
    for (const c of list) {
      host.appendChild(renderComicCard(c, c.id));
    }
  } catch (err) { toast('加载本地库存失败: ' + err.message, 'error'); }
}
$('#dl-rebuild').addEventListener('click', () => {
  API.post('/download/update-downloaded').then(() => {
    toast('已重建', 'success'); loadDownloaded();
  }).catch(err => toast('失败: ' + err.message, 'error'));
});

// ----------- 配置 -----------
const CONFIG_SCHEMA = [
  { key: 'downloadDir', label: '下载根目录', type: 'text',
    hint: '漫画下载到磁盘的根目录。容器里建议挂到 /downloads 这样的卷,避免容器销毁后丢失文件。' },
  { key: 'exportDir', label: '导出根目录', type: 'text',
    hint: '导出 PDF / CBZ 的根目录。相对路径以 downloadDir 为基准。' },
  { key: 'downloadFormat', label: '图片格式', type: 'select',
    options: [{v:'Jpeg',l:'Jpeg'},{v:'Webp',l:'Webp'},{v:'Png',l:'Png'}],
    hint: 'Jpeg: 有损(肉眼看不出)、体积最小、编码最快;宽高上限 65534(条漫可能超限报错)。\nWebp: 无损、体积约为 jpg 的 4 倍、宽高上限 16383。\nPng: 无损、体积约为 jpg 的 5 倍、编码最慢。' },
  { key: 'dirFmt', label: '目录命名格式', type: 'text',
    hint: '用 / 分隔目录层级;至少要两层 (倒数第二层放漫画元数据,最后一层放章节元数据)。\n可用字段: comic_id / chapter_id / comic_title / chapter_title / author / order。\n例: {author}/[{author}] {comic_title}({comic_id})/{order} - {chapter_title}' },
  { key: 'proxyMode', label: '代理模式', type: 'select',
    options: [{v:'System',l:'系统'},{v:'Disable',l:'不使用'},{v:'Http',l:'Http 代理'}] ,
    hint: 'System = 走系统代理;Disable = 直连 (不经过任何代理);Http = 用下面填的代理地址。' },
  { key: 'proxyHost', label: '代理地址', type: 'text',
    hint: '代理模式选 Http 时生效,例如 127.0.0.1。' },
  { key: 'proxyPort', label: '代理端口', type: 'number',
    hint: '代理模式选 Http 时生效,例如 7890 (Clash 默认)。' },
  { key: 'enableFileLogger', label: '文件日志', type: 'bool',
    hint: '开启后日志同时写到 /config/logs 下的滚动日志文件,方便事后排查;关闭后只输出到 stdout。' },
  { key: 'chapterConcurrency', label: '章节并发', type: 'number',
    hint: '同时下载的章节数。改完需要重启 server 才能生效。值越大占用带宽越多,可能被 JM 风控。' },
  { key: 'chapterDownloadIntervalSec', label: '章节间隔(秒)', type: 'number',
    hint: '每个章节下载完成后休息多久再开始下一个,用来缓解反爬。0 = 不休息。' },
  { key: 'imgConcurrency', label: '图片并发', type: 'number',
    hint: '同一个章节内同时下载的图片数。改完需要重启 server 才能生效。' },
  { key: 'imgDownloadIntervalSec', label: '图片间隔(秒)', type: 'number',
    hint: '每张图片下载完成后休息多久再开始下一张。0 = 不休息。' },
  { key: 'shouldDownloadCover', label: '下载封面', type: 'bool',
    hint: '开启后会在每本漫画目录里多下载一张 cover.jpg,用于本地阅读器/Kavita 识别。' },
  { key: 'apiDomainMode', label: 'API 域名', type: 'select',
    options: [
      {v:'Domain1',l:'Domain 1'},{v:'Domain2',l:'Domain 2'},
      {v:'Domain3',l:'Domain 3'},{v:'Domain4',l:'Domain 4'},{v:'Domain5',l:'Domain 5'},
    ],
    hint: 'JM 有 5 条 API 线路,如果某条线路 502/超时切到其他线路试试。切换后可能需要重新登录。' },
  { key: 'customApiDomain', label: '自定义 API 域名', type: 'text',
    hint: '把 API 域名改成上面 5 条以外的镜像,例如自建反代。' },
  { key: 'chineseNormalization', label: '简繁归一化', type: 'select',
    options: [{v:'None',l:'不转换'},{v:'ToSimplified',l:'转简体'},{v:'ToTraditional',l:'转繁体'}],
    hint: 'None: 保留网站原文 (简/繁/日混在一起,同一本漫画可能开多个目录)。\nToSimplified: 默认,转简体避免同本漫画开多目录。\nToTraditional: 转繁体。OpenCC 不会动韩文/日文假名/英文/数字。' },
  { key: 'missingImageThreshold', label: '缺失图片容忍', type: 'number',
    hint: '章节下载结束时缺失图片数 ≤ 此值视为下载成功 (仅日志告警),不会整章作废。\n设为 0 维持原行为: 缺一张就整章失败,需手动重试整章。\n失败的图片索引会写入日志 (搜索 chapter-download-warning / chapter-download-failure)。' },
  { key: 'chapterArchiveFormat', label: '章节归档', type: 'select',
    options: [{v:'None',l:'不打包'},{v:'Zip',l:'.zip'},{v:'Cbz',l:'.cbz'}],
    hint: 'None: 保留章节目录,不做额外处理。\nZip: 下载完成后把章节目录打包为 .zip 再删除原目录,导出 PDF/CBZ 时自动解压。\nCbz: 打包为 .cbz (漫画阅读器约定格式),适合只在本地用阅读器看的场景。' },
  { key: 'exportSkipMode', label: '导出跳过', type: 'select',
    options: [{v:'None',l:'不跳过'},{v:'SkipDownloaded',l:'跳过已下载'}] ,
    hint: '只影响「本地库存」里直接导出整部作品时的行为。「章节详情」里手动勾选导出时一律不跳过,每次重新导出。' },
];
async function loadConfig() {
  try {
    State.config = await API.get('/config');
    renderConfig();
  } catch (err) { toast('加载配置失败: ' + err.message, 'error'); }
}
function renderConfig() {
  const host = $('#config-form');
  host.innerHTML = '';
  for (const f of CONFIG_SCHEMA) {
    const v = State.config[f.key];
    let input;
    if (f.type === 'bool') {
      input = el('input', { type: 'checkbox' });
      input.checked = !!v;
    } else if (f.type === 'select') {
      input = el('select');
      for (const o of f.options) {
        const opt = el('option', { value: o.v }, o.l);
        if (String(v) === String(o.v)) opt.selected = true;
        input.appendChild(opt);
      }
    } else {
      input = el('input', { type: f.type, value: v ?? '' });
    }
    input.dataset.key = f.key;
    input.dataset.type = f.type;
    // hint 文本: 多行用 \n 分隔,渲染时转 <br>
    const hintNode = f.hint
      ? el('p', { class: 'hint', html: escapeHtml(f.hint).replace(/\n/g, '<br>') })
      : null;
    host.appendChild(el('div', { class: 'field' },
      el('label', {}, f.label),
      input,
      hintNode,
    ));
  }
  host.appendChild(el('div', { class: 'actions' },
    el('button', { class: 'btn primary', onclick: saveConfig }, '保存'),
    el('button', { class: 'btn ghost', onclick: loadConfig }, '放弃改动'),
  ));
}
async function saveConfig() {
  const cfg = { ...State.config };
  $$('#config-form [data-key]').forEach(inp => {
    const k = inp.dataset.key;
    const t = inp.dataset.type;
    if (t === 'bool') cfg[k] = inp.checked;
    else if (t === 'number') cfg[k] = Number(inp.value);
    else cfg[k] = inp.value;
  });
  try {
    await API.post('/config', cfg);
    State.config = cfg;
    toast('配置已保存', 'success');
  } catch (err) { toast('保存失败: ' + err.message, 'error'); }
}

// ----------- 登录 -----------
$('#login-btn').addEventListener('click', () => {
  $('#login-modal').classList.remove('hidden');
  $('#login-username').focus();
});
$('#login-cancel').addEventListener('click', () => $('#login-modal').classList.add('hidden'));
$('#login-submit').addEventListener('click', async () => {
  const username = $('#login-username').value.trim();
  const password = $('#login-password').value;
  if (!username || !password) { $('#login-error').textContent = '账号密码必填'; return; }
  $('#login-error').textContent = '';
  try {
    const profile = await API.post('/login', { username, password });
    State.user = profile;
    renderUser();
    $('#login-modal').classList.add('hidden');
    $('#login-password').value = '';
    toast(`欢迎, ${username}`, 'success');
  } catch (err) { $('#login-error').textContent = err.message; }
});
$('#logout-btn').addEventListener('click', () => {
  // server 没有 logout endpoint, 把 config 里的 username/password 清空
  // 这里简单地刷新页面
  if (!confirm('注销会清空当前页面的登录态,确认?')) return;
  State.user = null;
  renderUser();
});
function renderUser() {
  if (State.user) {
    $('#user-info').classList.remove('hidden');
    $('#login-btn').classList.add('hidden');
    $('#user-name').textContent = State.user.username || State.user.name || '';
    const photo = State.user.photo || State.user.photo_url || '';
    if (photo) {
      $('#user-photo').src = photo;
      $('#user-photo').classList.remove('hidden');
    }
  } else {
    $('#user-info').classList.add('hidden');
    $('#login-btn').classList.remove('hidden');
  }
}

// 自动尝试拉取已登录 profile
async function tryLoadProfile() {
  try {
    const p = await API.get('/user-profile');
    if (p && p.username) { State.user = p; renderUser(); }
  } catch { /* 未登录, 忽略 */ }
}

// ----------- 进度面板 + SSE -----------
$('#progress-toggle').addEventListener('click', () => {
  const p = $('#progress-panel');
  p.classList.toggle('collapsed');
  $('#progress-toggle').textContent = p.classList.contains('collapsed') ? '展开' : '收起';
});
function startSSE() {
  const es = new EventSource('/events');
  // 解开 server 的 {event, data} 嵌套
  // Rust 端: #[serde(tag = "event", content = "data")] enum DownloadEvent =>
  //   { "event": "TaskCreate", "data": { state, comic, chapter_info, ... } }
  // webui 期望的字段都在 data 里,所以把 envelope 拆掉再交给 handleEvent
  const unwrap = (raw) => {
    try {
      const p = JSON.parse(raw);
      if (p && typeof p === 'object' && p.data && typeof p.data === 'object') {
        return p.data;
      }
      return p;
    } catch { return null; }
  };
  es.onmessage = (e) => {
    const payload = unwrap(e.data);
    if (payload) handleEvent(null, payload);
  };
  // server 只发一个 named event:"download-event"(SSE event: 那一行)
  const names = ['download-event'];
  for (const n of names) {
    es.addEventListener(n, e => {
      const payload = unwrap(e.data);
      if (payload) handleEvent(n, payload);
    });
  }
  es.onerror = () => { /* 暂时忽略, EventSource 会自动重连 */ };
}
function handleEvent(name, payload) {
  if (!payload) return;
  // 容忍不同 event 命名 (snake / camel)
  // chapter_info 嵌套结构里 chapterId/chapterTitle 也是 camelCase
  const comicId = payload.comic_id ?? payload.comicId ?? payload.comic?.id;
  const chapterId = payload.chapter_id ?? payload.chapterId
                 ?? payload.chapter_info?.chapter_id ?? payload.chapter_info?.chapterId
                 ?? payload.chapter?.chapter_id;
  if (chapterId == null && comicId == null) return;
  const key = taskKey(comicId, chapterId);
  const downloaded = payload.downloaded_img_count ?? payload.downloadedImgCount
                  ?? payload.downloaded ?? 0;
  const total = payload.total_img_count ?? payload.totalImgCount ?? payload.total ?? 0;
  // 兼容多种写法: server 序列化为字符串 "Pending" / "Downloading" / "Failed" / "Completed" / "Paused"
  const state = payload.state ?? payload.task_state ?? payload.status ?? '';
  // 标题兼容链:
  //   TaskCreate.payload.comic.name        —— 漫画标题(Comic 字段叫 name)
  //   TaskCreate.payload.chapter_info.chapter_title/chapterTitle
  //   TaskUpdate 没 comic,沿用上次记录的 title
  const prior = State.tasks.get(key);
  const title = payload.comic?.name ?? payload.comic_title ?? payload.comicTitle
              ?? payload.chapter_info?.chapter_title ?? payload.chapter_info?.chapterTitle
              ?? payload.chapter_title ?? payload.chapterTitle
              ?? prior?.title
              ?? `Task ${chapterId ?? ''}`;
  const t = { title, downloaded, total, state, comicId, chapterId };
  if (state === 'Completed' || state === 'completed') {
    // 短暂保留以便用户看到完成态, 然后 3s 后清掉
    State.tasks.set(key, t);
    renderProgress();
    setTimeout(() => {
      const cur = State.tasks.get(key);
      if (cur === t) { State.tasks.delete(key); renderProgress(); }
    }, 3000);
  } else if (state === 'Failed' || state === 'failed') {
    State.tasks.set(key, { ...t, state: 'failed' });
    renderProgress();
    toast(`下载失败: ${t.title}`, 'error', 6000);
  } else if (state === 'Deleted' || state === 'deleted') {
    State.tasks.delete(key);
    renderProgress();
  } else {
    State.tasks.set(key, t);
    renderProgress();
  }
}
function renderProgress() {
  const host = $('#progress-body');
  host.innerHTML = '';
  const list = Array.from(State.tasks.values());
  $('#progress-count').textContent = `${list.length} 进行中`;
  if (list.length === 0) {
    host.appendChild(el('p', { class: 'muted small', id: 'progress-empty' }, '暂无下载'));
    return;
  }
  for (const t of list) {
    const pct = t.total > 0 ? Math.round((t.downloaded / t.total) * 100) : 0;
    // 状态显示文案: 跟 server 的 DownloadTaskState 对齐 (Pending/Downloading/Paused/Completed/Failed)
    let statusLabel, statusClass = '';
    const s = (t.state || '').toLowerCase();
    if (s === 'completed') {
      statusLabel = t.total > 0 ? `${t.downloaded}/${t.total} (100%)` : '完成';
    } else if (s === 'failed') {
      statusLabel = '失败';
      statusClass = 'failed';
    } else if (s === 'paused') {
      statusLabel = '已暂停';
      statusClass = 'paused';
    } else if (s === 'downloading') {
      // 下载中: 有 total 就显示比例,否则显示「准备中」表示正在等第一张图
      statusLabel = t.total > 0 ? `${t.downloaded}/${t.total} (${pct}%)` : '准备中';
    } else if (s === 'pending') {
      statusLabel = '排队中';
      statusClass = 'pending';
    } else {
      statusLabel = t.total > 0 ? `${t.downloaded}/${t.total} (${pct}%)` : '准备中';
    }
    const row = el('div', { class: `progress-row ${statusClass}` },
      el('span', { class: 'progress-title', title: t.title },
        t.title.length > 30 ? t.title.slice(0, 30) + '…' : t.title),
      el('span', { class: 'progress-count' }, statusLabel),
      el('div', { class: 'bar' }, el('div', { class: 'bar-fill' })),
    );
    row.querySelector('.bar-fill').style.width = pct + '%';
    host.appendChild(row);
  }
}

// ----------- 日志 viewer -----------
$('#logs-btn')?.addEventListener('click', openLogsModal);
// 日志按钮没在 topbar 加, 这里顺便在 user-area 旁边补一个
const logsBtn = el('button', { class: 'btn ghost', id: 'logs-btn' }, '日志');
logsBtn.addEventListener('click', openLogsModal);
$('.user-area').insertBefore(logsBtn, $('#login-btn'));
$('#logs-close').addEventListener('click', () => $('#logs-modal').classList.add('hidden'));
$('#logs-refresh').addEventListener('click', refreshLogs);
async function openLogsModal() {
  $('#logs-modal').classList.remove('hidden');
  try {
    const list = await API.get('/logs/list');
    const sel = $('#logs-file');
    sel.innerHTML = '';
    for (const f of list) {
      sel.appendChild(el('option', { value: f.name },
        `${f.name}  (${(f.size/1024).toFixed(1)} KB)`));
    }
    if (list.length > 0) refreshLogs();
    else $('#logs-content').textContent = '(暂无日志)';
  } catch (err) { $('#logs-content').textContent = '加载日志列表失败: ' + err.message; }
}
async function refreshLogs() {
  const name = $('#logs-file').value;
  if (!name) return;
  try {
    const data = await API.get('/logs/content?path=' + encodeURIComponent(name) + '&lines=300');
    $('#logs-content').textContent = data.content || '(空)';
    const pre = $('#logs-content');
    pre.scrollTop = pre.scrollHeight;
  } catch (err) { $('#logs-content').textContent = '读取失败: ' + err.message; }
}

// ----------- 启动 -----------
(async function init() {
  renderUser();
  await tryLoadProfile();
  startSSE();
  renderProgress();
  // 默认进 search tab
  switchTab('search');
  $('#search-input').focus();
})();
