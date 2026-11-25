import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it, expect, afterAll } from 'vitest';

/**
 * Acceptance checks for scripts/secret-scan.sh (Issue #1533).
 *
 * The script is the single local entry point for gitleaks: it is invoked by
 * the husky pre-commit hook and by `npm run secret:scan`. These tests pin
 * down its CLI contract (help, exit codes, config/binary handling) and add a
 * repo-wide acceptance scan proving the working tree contains no findings.
 */

const ROOT = process.cwd();
const SCRIPT = join(ROOT, 'scripts', 'secret-scan.sh');
const CONFIG = join(ROOT, '.gitleaks.toml');

const tempDirs: string[] = [];

function makeTempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

interface RunResult {
  code: number;
  stdout: string;
  stderr: string;
}

function runScript(args: string[], env: NodeJS.ProcessEnv = {}): RunResult {
  try {
    const stdout = execFileSync('bash', [SCRIPT, ...args], {
      encoding: 'utf-8',
      env: { ...process.env, ...env },
    });
    return { code: 0, stdout, stderr: '' };
  } catch (error) {
    const err = error as { status?: number; stdout?: string; stderr?: string };
    return { code: err.status ?? 1, stdout: err.stdout ?? '', stderr: err.stderr ?? '' };
  }
}

afterAll(() => {
  for (const dir of tempDirs) {
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {
      // ignore cleanup errors
    }
  }
});

describe('scripts/secret-scan.sh', () => {
  describe('script contract', () => {
    it('exists and is executable', () => {
      expect(existsSync(SCRIPT)).toBe(true);
      expect(statSync(SCRIPT).mode & 0o111).not.toBe(0);
    });

    it('starts with a bash shebang', () => {
      const firstLine = readFileSync(SCRIPT, 'utf-8').split('\n')[0];
      expect(firstLine).toMatch(/^#!/);
      expect(firstLine).toContain('bash');
    });

    it('--help prints usage and exit codes, and exits 0', () => {
      const result = runScript(['--help']);
      expect(result.code).toBe(0);
      expect(result.stdout).toContain('Usage: scripts/secret-scan.sh');
      expect(result.stdout).toContain('--history');
      expect(result.stdout).toContain('Exit codes:');
    });

    it('rejects unknown options with exit code 2', () => {
      const result = runScript(['--bogus-flag']);
      expect(result.code).toBe(2);
      expect(result.stderr).toContain('unknown option: --bogus-flag');
    });

    it('fails with exit code 2 when the config file is missing', () => {
      const result = runScript(['--config', '/nonexistent/.gitleaks.toml']);
      expect(result.code).toBe(2);
      expect(result.stderr).toContain('gitleaks config not found');
    });

    it('fails with exit code 2 and install hints when gitleaks is missing', () => {
      const result = runScript([], { GITLEAKS_BIN: '/nonexistent/gitleaks-binary' });
      expect(result.code).toBe(2);
      expect(result.stderr).toContain('gitleaks not found');
      expect(result.stderr).toContain('brew install gitleaks');
    });
  });

  describe('scan behaviour', () => {
    it('exits 0 on a clean directory', () => {
      const cleanDir = makeTempDir('secret-scan-clean-');
      writeFileSync(join(cleanDir, 'readme.md'), '# nothing secret here\n');
      const result = runScript(['--source', cleanDir, '--config', CONFIG]);
      expect(result.code).toBe(0);
      expect(result.stdout).toContain('no secrets found');
    });

    it('exits 1 and reports the rule when a secret is present', () => {
      const leakyDir = makeTempDir('secret-scan-leak-');
      const reportPath = join(leakyDir, 'report.json');
      // S + 55 base58-ish characters: matches the repo's stellar-secret-key rule.
      const fakeKey = `S${'abcABC123'.repeat(7).slice(0, 55)}`;
      writeFileSync(join(leakyDir, 'leak.env'), `STELLAR_SECRET_KEY=${fakeKey}\n`);

      const result = runScript([
        '--source',
        leakyDir,
        '--config',
        CONFIG,
        '--report',
        reportPath,
      ]);
      expect(result.code).toBe(1);
      expect(result.stderr).toContain('secrets found');

      expect(existsSync(reportPath)).toBe(true);
      const findings = JSON.parse(readFileSync(reportPath, 'utf-8'));
      expect(Array.isArray(findings)).toBe(true);
      expect(findings.length).toBeGreaterThan(0);
      expect(findings[0].RuleID).toBe('stellar-secret-key');
    });

    it(
      'acceptance: repo working tree contains no secrets (findings fixed)',
      () => {
        const reportPath = join(makeTempDir('secret-scan-repo-'), 'repo-report.json');
        const result = runScript(['--config', CONFIG, '--report', reportPath]);
        expect(
          result.code,
          `secret-scan failed:\n${result.stdout}\n${result.stderr}`,
        ).toBe(0);
        expect(result.stdout).toContain('no secrets found');
        expect(JSON.parse(readFileSync(reportPath, 'utf-8'))).toEqual([]);
      },
      { timeout: 120_000 },
    );
  });
});
