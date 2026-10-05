/** 新标签打开 /tests/workspace.html#/browse/home；只使用离线数据，不创建真实任务。 */
import { nextTick } from "vue";
import router from "../src/router";
import { goBack } from "../src/router/navigation";
import { useDownloadPanelStore } from "../src/stores/downloadPanel";
import { taskFailedCount, taskProgress, useTaskStore, type Task } from "../src/stores/tasks";
import { fillDownloadForm } from "../src/utils/pixivHooks";
import { fallbackPage } from "../src/router/navigation";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))); };
async function waitFor(check: () => boolean) {
  const end = Date.now() + 5000;
  while (!check()) { assert(Date.now() < end, "等待页面超时"); await new Promise(resolve => setTimeout(resolve, 20)); }
}
function task(id: string, status: string, total = 10): Task {
  return { task_id:id, source_type:"single", source_id:id, category:"novel", status, total, done:3, skipped:1, failed_ids:"[]", created_at:id, updated_at:id, error:null };
}

async function run() {
  await router.isReady(); await waitFor(() => !!document.querySelector('.work-card'));
  const panel = useDownloadPanelStore(); const tasks = useTaskStore();
  assert(document.querySelectorAll('.sidebar .nav-item').length === 4, '只有四个核心入口');
  assert(document.querySelector('.browse-home .browse-navigation a[aria-current="page"]')?.getAttribute('href') === '#/browse/home', '分区导航属于列表页并标明当前页面');
  assert(document.querySelector('.download-status')?.getBoundingClientRect().height === 32, '底部状态栏为32px独立行');
  const main = document.querySelector<HTMLElement>('.app-content')!;
  main.scrollTop = 300; await settle();
  const top = main.scrollTop;
  const first = document.querySelector('.work-card');
  const path = router.currentRoute.value.fullPath;
  const historyPosition = router.options.history.state.position;
  for (const [form, sourceType] of [['novel','single'], ['novel','series'], ['novel','user'], ['illustration','single'], ['illustration','user']] as const) {
    fillDownloadForm({ form, sourceType, sourceId:42 }); await settle();
    assert(panel.visible && panel.kind === form && panel.drafts[form].sourceType === sourceType, '所有来源就地返填');
    assert(router.currentRoute.value.fullPath === path && router.options.history.state.position === historyPosition, '面板不改变URL与路由历史');
    assert(document.querySelector('.work-card') === first, '展开面板不重挂浏览列表');
    panel.close(); await settle();
  }
  assert(Math.abs(main.scrollTop-top) < 2, '面板关闭恢复列表滚动');
  panel.open({ form:'novel', sourceType:'single', sourceId:42 });
  panel.drafts.novel.sourceId='123'; panel.drafts.novel.edited=true; panel.drafts.novel.formats=['markdown'];
  panel.open({ form:'novel', sourceType:'series', sourceId:77 }); await settle();
  assert(panel.pendingTarget && document.querySelector('.download-panel dialog[open]'), '覆盖手动来源先确认');
  panel.pendingTarget=null; await settle();
  assert(panel.drafts.novel.sourceId==='123', '取消保留原稿');
  panel.open({ form:'novel', sourceType:'series', sourceId:77 }); await settle(); panel.confirmTarget(); await settle();
  assert(panel.drafts.novel.sourceId==='77' && panel.drafts.novel.formats[0]==='markdown', '确认只替换来源，保留格式');
  panel.close(); panel.open(); await settle(); assert(panel.drafts.novel.sourceId==='77', '关闭重开保留草稿');
  window.dispatchEvent(new KeyboardEvent('keydown',{ key:'Escape', bubbles:true })); await settle(); assert(!panel.visible,'Escape关闭面板');
  panel.open(); await router.push('/browse/feed'); await settle(); assert(!panel.visible && panel.drafts.novel.sourceId==='77','页面导航关闭但保留草稿');
  await router.push('/browse/work/illust/9000005'); await settle();
  assert(document.querySelector('.nav-item.active')?.getAttribute('aria-label')==='关注','详情继承关注来路');
  goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/feed');
  goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/home'); await settle();
  assert(document.querySelector('.work-card')===first && Math.abs(main.scrollTop-top)<2,'详情与跨分区返回恢复缓存和位置');
  // 插画详情页进 KeepAlive：作者页与相关推荐往返复用同一实例，不再重新加载
  await router.push('/browse/work/illust/9000005'); await waitFor(() => !!document.querySelector('.work-view .stage-scroll')); await settle();
  const cachedWork = document.querySelector('.work-view');
  assert(!!document.querySelector('.work-view .author-name')?.textContent,'详情加载出作者行');
  await router.push('/browse/user/2'); await waitFor(() => router.currentRoute.value.path==='/browse/user/2'); await settle();
  window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})); await settle();
  assert(router.currentRoute.value.path==='/browse/user/2','详情停用后不再响应 Esc 返回');
  goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/work/illust/9000005'); await settle();
  assert(document.querySelector('.work-view')===cachedWork && !document.querySelector('.work-view .sk-line'),'从作者页返回复用已加载详情，不重新加载');
  await router.push('/browse/work/illust/9000006'); await waitFor(() => !!document.querySelector('.work-view .stage-scroll') && document.querySelector('.work-view')!==cachedWork); await settle();
  goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/work/illust/9000005'); await settle();
  assert(document.querySelector('.work-view')===cachedWork,'相关推荐回退复用已加载详情，不重新加载');
  await router.push('/browse/home'); await waitFor(() => router.currentRoute.value.path==='/browse/home'); await settle();
  window.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true})); await waitFor(() => router.currentRoute.value.path==='/browse/search'); await settle();
  assert(!document.querySelector('.search-fab') && document.activeElement?.classList.contains('search-field'),'Ctrl+K进入搜索并聚焦，搜索页无重复胶囊');
  assert(document.body.innerText.includes('以图识图'),'搜索页提供以图识图');
  goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/home');
  await router.push('/illustration?sourceType=user&sourceId=12&keep=yes#anchor'); await waitFor(() => router.currentRoute.value.path==='/tools/tasks' && !router.currentRoute.value.query.downloadForm); await settle();
  assert(panel.visible && panel.kind==='illustration' && panel.drafts.illustration.sourceId==='12','旧路径打开正确的表单');
  assert(document.querySelectorAll('.tools-view h1').length===1,'下载页只有一个主标题，页签不重复生成标题');
  assert(router.currentRoute.value.query.keep==='yes' && router.currentRoute.value.hash==='#anchor','旧路径保留其他query与hash');
  assert(fallbackPage('/browse/watchlist')==='/browse/feed' && fallbackPage('/browse/history')==='/browse/bookmark' && fallbackPage('/tools/history')==='/tools/tasks','无历史回所属核心入口');
  panel.close(); goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/home'); await settle();
  await router.push('/browse/work/novel/9000005'); await waitFor(() => !!document.querySelector('.novel-view'));
  panel.open({form:'novel',sourceType:'single',sourceId:42}); await settle();
  const readerRect=document.querySelector('.novel-view')!.getBoundingClientRect();
  const paneRect=document.querySelector('.main-pane')!.getBoundingClientRect();
  assert(Math.abs(readerRect.height-paneRect.height)<2 && readerRect.bottom<=paneRect.bottom+1,'阅读器按可用工作区布局，底部控件不被面板遮挡');
  panel.close(); goBack(router); await waitFor(() => router.currentRoute.value.path==='/browse/home'); await settle();

  // 在应用已以浏览器mock挂载后，短暂模拟任务IPC与事件；不触碰真实后端。
  let snapshot = [task('2','pending'),task('3','running'),task('1','running'),task('4','paused')];
  let listCalls=0; let createCalls=0; let browseRefreshCalls=0; let failList=false; let rejectCreate=true;
  const callbacks = new Map<number,(event:unknown)=>void>(); const subscriptions = new Map<string,number>(); let callbackId=0;
  const internals = {
    transformCallback: (fn:(event:unknown)=>void) => { callbacks.set(++callbackId,fn); return callbackId; },
    invoke: async (command:string,args:Record<string,unknown>={}) => {
      if(command==='tasks_list') { listCalls++; if(failList) throw new Error('offline'); return {items:snapshot}; }
      if(command==='browse_home_feed') { browseRefreshCalls++; return {items:[]}; }
      if(command==='plugin:event|listen') { subscriptions.set(String(args.event),Number(args.handler)); return args.handler; }
      if(command==='plugin:event|unlisten') { subscriptions.delete(String(args.event)); return null; }
      if(command==='task_create') { createCalls++; await new Promise(resolve=>setTimeout(resolve,40)); return rejectCreate?{error:'测试创建失败'}:{task_id:'new'}; }
      return {};
    },
  };
  window.__TAURI_INTERNALS__=internals;
  Object.assign(window,{__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  const stop = tasks.startMonitoring(); assert(tasks.startMonitoring()===stop,'监控只启动一次');
  await waitFor(() => tasks.tasks.length===4 && subscriptions.size===2);
  panel.open(); await settle();
  const sourceField=document.querySelector<HTMLElement>('.download-panel md-outlined-text-field')!;
  sourceField.focus(); sourceField.dispatchEvent(new KeyboardEvent('keydown',{key:'r',ctrlKey:true,bubbles:true,composed:true})); await settle();
  assert(browseRefreshCalls===0,'面板输入聚焦时刷新快捷键不刷新背后列表');
  document.querySelector<HTMLElement>('.list-refresh')!.click(); await waitFor(()=>browseRefreshCalls===1);
  assert(panel.visible,'手动刷新仍可用且不关闭下载面板');
  panel.close();
  assert(tasks.representative?.task_id==='1' && taskProgress(tasks.representative)===0.3,'代表任务优先运行中再按创建时间，进度done/total');
  snapshot=[task('unknown','pending',0)]; callbacks.get(subscriptions.get('task://progress')!)?.({payload:{}}); await waitFor(()=>tasks.representative?.task_id==='unknown'); await settle();
  assert(document.querySelector<HTMLElement & {indeterminate:boolean}>('.mini-progress')?.indeterminate,'未知总量显示不定进度');
  snapshot=[task('paused','paused',0)]; callbacks.get(subscriptions.get('task://progress')!)?.({payload:{}}); await waitFor(()=>tasks.representative?.status==='paused'); await settle();
  assert(!document.querySelector('.mini-progress')?.hasAttribute('indeterminate'),'暂停停止不定进度动画');
  assert(taskFailedCount({...task('bad','done'),failed_ids:'invalid'})===0,'损坏失败列表不使状态栏崩溃');
  failList=true; await tasks.fetchTasks().catch(()=>{}); await settle();
  assert(tasks.syncFailed && tasks.tasks.length===1 && document.querySelector('.download-status')?.textContent?.includes('正在重试'),'同步失败保留快照并回显');
  failList=false; await tasks.fetchTasks();
  const beforePoll=listCalls; await new Promise(resolve=>setTimeout(resolve,2100)); assert(listCalls>beforePoll,'不在下载页也有轮询兜底');
  panel.open({form:'novel',sourceType:'single',sourceId:99}); await settle();
  const submit=document.querySelector<HTMLElement>('.download-panel form md-filled-button')!;
  submit.click(); submit.click(); await waitFor(()=>!panel.submitting); await settle();
  assert(createCalls===1 && panel.visible && panel.error==='测试创建失败' && panel.drafts.novel.sourceId==='99','失败保留输入并阻止重复提交');
  rejectCreate=false; submit.click(); await waitFor(()=>!panel.submitting); await settle();
  assert(!panel.visible && router.currentRoute.value.path==='/browse/home','成功关闭面板不跳页');
  stop(); await settle(); assert(subscriptions.size===0,'应用监控可完整清理');
  delete window.__TAURI_INTERNALS__;
  document.querySelector<HTMLElement>('.list-refresh')!.click(); await waitFor(()=>!!document.querySelector('.work-card'));
  panel.reset(); assert(panel.drafts.novel.sourceId==='' && panel.drafts.illustration.sourceId==='' && !panel.visible,'账号重置清空两类草稿');
  tasks.tasks=[]; tasks.syncFailed=false;
  window.dispatchEvent(new CustomEvent('pixiv-tool:image-fullscreen',{detail:true})); await settle();
  assert(getComputedStyle(document.querySelector('.download-status')!).display==='none' && getComputedStyle(document.querySelector('.search-fab')!).display==='none','全屏看图隐藏全局状态栏和搜索');
  window.dispatchEvent(new CustomEvent('pixiv-tool:image-fullscreen',{detail:false})); await settle();
  panel.open({form:'novel',sourceType:'single',sourceId:9000005}); await settle();
  const output=document.querySelector<HTMLOutputElement>('#workspace-result')!;
  output.value='PASS：四入口、返填与草稿、缓存返回、详情复用、搜索、旧链接、后台任务事件与轮询、失败与重复提交'; output.dataset.result='pass';
}
run().catch(error=>{const output=document.querySelector<HTMLOutputElement>('#workspace-result')!;output.value=`FAIL：${String(error)}`;output.dataset.result='fail';console.error(error);});
