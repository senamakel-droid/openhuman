const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const readline = require('node:readline');

let chromium;
try {
  ({ chromium } = require('playwright'));
} catch (error) {
  ({ chromium } = require('@playwright/test'));
}

const userDataDir =
  process.env.OPENHUMAN_PLAYWRIGHT_USER_DATA_DIR ||
  path.join(os.tmpdir(), 'openhuman-playwright-browser');
const headless = process.env.OPENHUMAN_PLAYWRIGHT_HEADLESS !== '0';

let context = null;
let page = null;
let refs = new Map();

function write(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

async function ensurePage() {
  if (!context) {
    fs.mkdirSync(userDataDir, { recursive: true });
    context = await chromium.launchPersistentContext(userDataDir, {
      headless,
      viewport: { width: 1365, height: 900 },
    });
  }

  if (!page || page.isClosed()) {
    page = context.pages()[0] || (await context.newPage());
  }

  return page;
}

function cssPath(el) {
  if (!el || el.nodeType !== Node.ELEMENT_NODE) return '';
  const parts = [];
  let node = el;
  while (node && node.nodeType === Node.ELEMENT_NODE && node !== document.body) {
    const tag = node.tagName.toLowerCase();
    if (node.id) {
      parts.unshift(`${tag}#${CSS.escape(node.id)}`);
      break;
    }
    const parent = node.parentElement;
    if (!parent) break;
    const siblings = Array.from(parent.children).filter(child => child.tagName === node.tagName);
    const index = siblings.indexOf(node) + 1;
    parts.unshift(siblings.length > 1 ? `${tag}:nth-of-type(${index})` : tag);
    node = parent;
  }
  return parts.length ? parts.join(' > ') : 'body';
}

async function snapshot(interactiveOnly, compact, depth) {
  return await page.evaluate(
    ({ interactiveOnly, compact, depth }) => {
      const isInteractive = el => {
        const tag = el.tagName.toLowerCase();
        const role = el.getAttribute('role');
        return (
          ['a', 'button', 'input', 'select', 'textarea', 'summary'].includes(tag) ||
          ['button', 'link', 'checkbox', 'menuitem', 'option', 'radio', 'switch', 'tab'].includes(role || '') ||
          el.hasAttribute('onclick') ||
          el.tabIndex >= 0
        );
      };

      const nameFor = el =>
        (el.getAttribute('aria-label') ||
          el.getAttribute('alt') ||
          el.getAttribute('title') ||
          el.getAttribute('placeholder') ||
          el.innerText ||
          el.textContent ||
          '')
          .replace(/\s+/g, ' ')
          .trim()
          .slice(0, 180);

      const cssPath = el => {
        if (!el || el.nodeType !== Node.ELEMENT_NODE) return '';
        const parts = [];
        let node = el;
        while (node && node.nodeType === Node.ELEMENT_NODE && node !== document.body) {
          const tag = node.tagName.toLowerCase();
          if (node.id) {
            parts.unshift(`${tag}#${CSS.escape(node.id)}`);
            break;
          }
          const parent = node.parentElement;
          if (!parent) break;
          const siblings = Array.from(parent.children).filter(child => child.tagName === node.tagName);
          const index = siblings.indexOf(node) + 1;
          parts.unshift(siblings.length > 1 ? `${tag}:nth-of-type(${index})` : tag);
          node = parent;
        }
        return parts.length ? parts.join(' > ') : 'body';
      };

      const elements = [];
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_ELEMENT);
      let node = walker.currentNode;
      let id = 1;
      while (node) {
        const rect = node.getBoundingClientRect();
        const visible = rect.width > 0 && rect.height > 0;
        const interactive = isInteractive(node);
        const name = nameFor(node);
        if (visible && (!interactiveOnly || interactive) && (!compact || name || interactive)) {
          elements.push({
            ref: `@e${id++}`,
            tag: node.tagName.toLowerCase(),
            role: node.getAttribute('role') || undefined,
            name,
            selector: cssPath(node),
          });
        }
        if (depth && elements.length >= depth * 80) break;
        node = walker.nextNode();
      }
      return {
        title: document.title,
        url: location.href,
        elements,
      };
    },
    { interactiveOnly, compact, depth },
  );
}

async function selectorFor(input) {
  if (typeof input === 'string' && input.startsWith('@')) {
    const selector = refs.get(input);
    if (!selector) throw new Error(`Unknown element ref: ${input}. Run snapshot first.`);
    return selector;
  }
  return input;
}

async function runFind(args) {
  const current = await ensurePage();
  const value = args.value || '';
  let locator;
  switch (args.by) {
    case 'role':
      locator = current.getByRole(value);
      break;
    case 'text':
      locator = current.getByText(value);
      break;
    case 'label':
      locator = current.getByLabel(value);
      break;
    case 'placeholder':
      locator = current.getByPlaceholder(value);
      break;
    case 'testid':
      locator = current.getByTestId(value);
      break;
    default:
      throw new Error(`Unsupported find locator: ${args.by}`);
  }

  const first = locator.first();
  switch (args.find_action) {
    case 'click':
      await first.click();
      return { action: 'find', by: args.by, find_action: 'click' };
    case 'fill':
      await first.fill(args.fill_value || '');
      return { action: 'find', by: args.by, find_action: 'fill' };
    case 'hover':
      await first.hover();
      return { action: 'find', by: args.by, find_action: 'hover' };
    case 'text':
      return { action: 'find', by: args.by, find_action: 'text', text: await first.innerText() };
    case 'check':
      await first.check();
      return { action: 'find', by: args.by, find_action: 'check' };
    default:
      throw new Error(`Unsupported find action: ${args.find_action}`);
  }
}

async function run(args) {
  const current = await ensurePage();
  switch (args.action) {
    case 'open':
      await current.goto(args.url, { waitUntil: 'domcontentloaded' });
      return { backend: 'playwright', action: 'open', url: current.url(), title: await current.title() };
    case 'snapshot': {
      const data = await snapshot(args.interactive_only ?? true, args.compact ?? true, args.depth);
      refs = new Map(data.elements.map(element => [element.ref, element.selector]));
      return { backend: 'playwright', action: 'snapshot', ...data };
    }
    case 'click':
      await current.locator(await selectorFor(args.selector)).first().click();
      return { backend: 'playwright', action: 'click', selector: args.selector };
    case 'fill':
      await current.locator(await selectorFor(args.selector)).first().fill(args.value || '');
      return { backend: 'playwright', action: 'fill', selector: args.selector };
    case 'type':
      await current.locator(await selectorFor(args.selector)).first().type(args.text || '');
      return { backend: 'playwright', action: 'type', selector: args.selector, typed: (args.text || '').length };
    case 'get_text':
      return {
        backend: 'playwright',
        action: 'get_text',
        selector: args.selector,
        text: await current.locator(await selectorFor(args.selector)).first().innerText(),
      };
    case 'get_title':
      return { backend: 'playwright', action: 'get_title', title: await current.title() };
    case 'get_url':
      return { backend: 'playwright', action: 'get_url', url: current.url() };
    case 'screenshot': {
      const png = await current.screenshot({ fullPage: Boolean(args.full_page) });
      if (args.path) {
        fs.mkdirSync(path.dirname(args.path), { recursive: true });
        fs.writeFileSync(args.path, png);
        return { backend: 'playwright', action: 'screenshot', path: args.path, bytes: png.length };
      }
      return { backend: 'playwright', action: 'screenshot', png_base64: png.toString('base64'), bytes: png.length };
    }
    case 'wait':
      if (args.selector) await current.locator(await selectorFor(args.selector)).first().waitFor();
      else if (args.text) await current.getByText(args.text).first().waitFor();
      else await current.waitForTimeout(args.ms || 1000);
      return { backend: 'playwright', action: 'wait' };
    case 'press':
      await current.keyboard.press(args.key);
      return { backend: 'playwright', action: 'press', key: args.key };
    case 'hover':
      await current.locator(await selectorFor(args.selector)).first().hover();
      return { backend: 'playwright', action: 'hover', selector: args.selector };
    case 'scroll':
      await current.mouse.wheel(
        args.direction === 'left' ? -(args.pixels || 600) : args.direction === 'right' ? args.pixels || 600 : 0,
        args.direction === 'up' ? -(args.pixels || 600) : args.direction === 'down' ? args.pixels || 600 : 0,
      );
      return { backend: 'playwright', action: 'scroll', direction: args.direction };
    case 'is_visible':
      return {
        backend: 'playwright',
        action: 'is_visible',
        selector: args.selector,
        visible: await current.locator(await selectorFor(args.selector)).first().isVisible(),
      };
    case 'find':
      return { backend: 'playwright', ...(await runFind(args)) };
    case 'close':
      if (context) await context.close();
      context = null;
      page = null;
      refs = new Map();
      return { backend: 'playwright', action: 'close' };
    default:
      throw new Error(`Unsupported action: ${args.action}`);
  }
}

const rl = readline.createInterface({ input: process.stdin });
rl.on('line', async line => {
  let request;
  try {
    request = JSON.parse(line);
    const data = await run(request.args || {});
    write({ id: request.id, success: true, data });
  } catch (error) {
    write({ id: request && request.id, success: false, error: error && error.message ? error.message : String(error) });
  }
});

process.on('SIGTERM', async () => {
  try {
    if (context) await context.close();
  } finally {
    process.exit(0);
  }
});
