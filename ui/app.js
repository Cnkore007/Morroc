const { invoke, listen } = window.__TAURI__?.core || {};

const tauriReady = typeof invoke === 'function';

const MAX_LOG_LINES = 500;

function logInfo(msg) {
  console.log(msg);
  appendLog(`[System] INFO ${msg}`);
}

// 导航切换
document.querySelectorAll('.nav-item').forEach(item => {
  item.addEventListener('click', (e) => {
    e.preventDefault();
    const view = item.dataset.view;

    document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
    item.classList.add('active');

    document.querySelectorAll('.view').forEach(v => v.classList.remove('active'));
    document.getElementById(`view-${view}`).classList.add('active');

    if (view === 'scripts') refreshScripts();
    if (view === 'accounts') refreshAccounts();
  });
});

// 服务端控制
const modeSelect = document.getElementById('mode-select');
const serverToggle = document.getElementById('server-toggle');
const serverStatusText = document.getElementById('server-status-text');

let serverRunning = true;
let serverUptimeSeconds = 0;

function formatUptime(seconds) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

function updateServerUI(status) {
  serverRunning = status.running;
  modeSelect.value = status.mode;
  serverToggle.textContent = status.running ? '停止' : '启动';
  serverToggle.classList.toggle('primary', !status.running);
  serverUptimeSeconds = status.uptime_seconds || 0;

  document.getElementById('uptime').textContent = formatUptime(serverUptimeSeconds);
  document.getElementById('online-count').textContent = status.sessions || 0;

  const modeName = status.mode === 'distributed' ? '分布式' : '单实例';
  const addrText = status.addresses && status.addresses.length
    ? status.addresses.join(', ')
    : '—';
  serverStatusText.textContent = status.running
    ? `运行中 | 模式: ${modeName} | 账户: ${status.accounts || 0} | 会话: ${status.sessions || 0}`
    : `已停止 | 模式: ${modeName}`;
  document.getElementById('status-detail').textContent = status.running
    ? `监听: ${addrText}`
    : '服务已停止';

  document.getElementById('status-text').textContent = status.running ? '已连接' : '未连接';
  const dot = document.getElementById('status-dot');
  dot.classList.toggle('online', status.running);
}

async function refreshServerStatus() {
  if (!tauriReady) {
    document.getElementById('status-text').textContent = '非 Tauri 环境';
    return;
  }
  try {
    const status = await invoke('get_server_status');
    updateServerUI(status);
  } catch (err) {
    console.error('获取服务状态失败:', err);
  }
}

async function refreshMetrics() {
  if (!tauriReady) return;
  try {
    const metrics = await invoke('get_system_metrics');
    document.getElementById('cpu-percent').textContent = `${metrics.cpu_percent.toFixed(1)}%`;
    document.getElementById('ram-percent').textContent = `${metrics.memory_percent.toFixed(1)}%`;
  } catch (err) {
    console.error('获取系统指标失败:', err);
  }
}

serverToggle.addEventListener('click', async () => {
  if (!tauriReady) {
    logInfo('当前不在 Tauri 环境中，无法调用后端。');
    return;
  }
  serverToggle.disabled = true;
  try {
    if (serverRunning) {
      await invoke('stop_server');
      logInfo('Morroc 服务已停止');
    } else {
      const mode = modeSelect.value;
      await invoke('start_server', { mode });
      logInfo(`Morroc 服务已启动（${mode === 'distributed' ? '分布式' : '单实例'}）`);
    }
    await refreshServerStatus();
  } catch (err) {
    logInfo(`操作失败: ${err}`);
  } finally {
    serverToggle.disabled = false;
  }
});

setInterval(refreshServerStatus, 2000);
setInterval(refreshMetrics, 3000);
setInterval(() => {
  if (serverRunning) {
    serverUptimeSeconds += 1;
    document.getElementById('uptime').textContent = formatUptime(serverUptimeSeconds);
  }
}, 1000);
refreshServerStatus();
refreshMetrics();

// Agent 聊天
const chatMessages = document.getElementById('chat-messages');
const agentInput = document.getElementById('agent-input');
const agentSend = document.getElementById('agent-send');

function addMessage(sender, text, isUser = false) {
  const msg = document.createElement('div');
  msg.className = `message ${isUser ? 'user' : ''}`;
  msg.innerHTML = `<strong>${sender}</strong>${escapeHtml(text)}`;
  chatMessages.appendChild(msg);
  chatMessages.scrollTop = chatMessages.scrollHeight;
}

agentSend.addEventListener('click', async () => {
  const text = agentInput.value.trim();
  if (!text) return;
  addMessage('你', text, true);
  agentInput.value = '';
  if (!tauriReady) {
    addMessage('Agent', '当前不在 Tauri 环境中，无法调用后端。');
    return;
  }
  try {
    const resp = await invoke('agent_chat', { message: text });
    let content = resp.content || '（无回复）';
    if (resp.tool_results && resp.tool_results.length > 0) {
      content += '\n[工具结果]\n' + resp.tool_results.map(t => {
        const ok = t.error ? `❌ ${t.error}` : JSON.stringify(t.result, null, 2);
        return `${t.name}: ${ok}`;
      }).join('\n');
    }
    addMessage('Agent', content);
  } catch (err) {
    addMessage('Agent', `调用失败: ${err}`);
  }
});

agentInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') agentSend.click();
});

// 脚本编辑器
const scriptSelect = document.getElementById('script-select');
const scriptName = document.getElementById('script-name');
const scriptEditor = document.getElementById('script-editor');
const saveScriptBtn = document.getElementById('save-script');

async function refreshScripts() {
  if (!tauriReady) return;
  try {
    const names = await invoke('list_scripts');
    const current = scriptSelect.value;
    scriptSelect.innerHTML = '<option value="">新建脚本</option>';
    names.forEach(name => {
      const opt = document.createElement('option');
      opt.value = name;
      opt.textContent = name;
      scriptSelect.appendChild(opt);
    });
    scriptSelect.value = current;
  } catch (err) {
    logInfo(`列出脚本失败: ${err}`);
  }
}

scriptSelect.addEventListener('change', async () => {
  const name = scriptSelect.value;
  if (!name) {
    scriptName.value = '';
    scriptEditor.value = '';
    return;
  }
  scriptName.value = name;
  if (!tauriReady) return;
  try {
    const content = await invoke('load_script', { name });
    scriptEditor.value = content || '';
  } catch (err) {
    logInfo(`加载脚本失败: ${err}`);
  }
});

saveScriptBtn.addEventListener('click', async () => {
  const name = scriptName.value.trim();
  const content = scriptEditor.value;
  if (!name) {
    addMessage('系统', '请输入脚本名', false);
    return;
  }
  if (!tauriReady) {
    addMessage('系统', '当前不在 Tauri 环境中。', false);
    return;
  }
  try {
    await invoke('save_script', { name, content });
    addMessage('系统', `脚本 ${name}.ro 已保存并热重载。`, false);
    await refreshScripts();
  } catch (err) {
    addMessage('系统', `保存脚本失败: ${err}`, false);
  }
});

// 账户管理
const accountList = document.getElementById('account-list');
const createAccountBtn = document.getElementById('create-account');

async function refreshAccounts() {
  if (!tauriReady) return;
  try {
    const accounts = await invoke('list_accounts');
    accountList.innerHTML = accounts.map(u => `<li class="list-item">${escapeHtml(u)}</li>`).join('');
  } catch (err) {
    logInfo(`列出账户失败: ${err}`);
  }
}

createAccountBtn.addEventListener('click', async () => {
  const userid = document.getElementById('new-userid').value.trim();
  const password = document.getElementById('new-password').value;
  const sex = document.getElementById('new-sex').value;
  if (!userid || !password) return;
  if (!tauriReady) return;
  try {
    const id = await invoke('create_account', { userid, password, sex });
    addMessage('系统', `账户 ${userid} 创建成功 (ID: ${id})`, false);
    document.getElementById('new-userid').value = '';
    document.getElementById('new-password').value = '';
    await refreshAccounts();
  } catch (err) {
    addMessage('系统', `创建账户失败: ${err}`, false);
  }
});

// 彩色日志监听
function appendLog(raw) {
  const logViewer = document.getElementById('log-viewer');
  if (!logViewer) return;

  const line = document.createElement('div');
  line.className = 'log-line';

  const match = raw.match(/^\[([^\]]+)\]\s+(\w+)\s+(.*)$/);
  if (match) {
    const [, tag, level, message] = match;
    const tagClass = `log-tag-${tag.toLowerCase()}`;
    const levelClass = `log-level-${level.toLowerCase()}`;
    line.innerHTML = `<span class="log-tag ${tagClass}">[${escapeHtml(tag)}]</span> <span class="log-level ${levelClass}">${escapeHtml(level)}</span> <span class="log-message">${escapeHtml(message)}</span>`;
  } else {
    line.textContent = raw;
  }

  logViewer.appendChild(line);

  while (logViewer.children.length > MAX_LOG_LINES) {
    logViewer.removeChild(logViewer.firstChild);
  }

  logViewer.scrollTop = logViewer.scrollHeight;
}

try {
  if (listen) {
    listen('log', (event) => {
      appendLog(event.payload);
    });
  }
} catch (err) {
  console.warn('监听日志事件失败:', err);
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/\n/g, '<br>');
}

refreshScripts();
refreshAccounts();
appendLog('[System] INFO 前端已加载，等待后端日志...');
