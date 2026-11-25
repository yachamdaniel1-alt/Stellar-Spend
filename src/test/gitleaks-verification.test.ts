import { execSync } from 'child_process';
import { existsSync, readFileSync, writeFileSync, unlinkSync, mkdtempSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';
import { describe, it, expect } from 'vitest';

const FIXTURE_DIR = join(__dirname, 'fixtures', 'secret-scan');
const GITLEAKS_CONFIG = join(process.cwd(), '.gitleaks.toml');
// Temp artifacts live in the OS tmpdir so they never land in the repo root
// (where they would race with repo-wide secret-scan acceptance tests).
const TEMP_DIR = mkdtempSync(join(tmpdir(), 'gitleaks-verification-'));
const TEMP_CONFIG_PATH = join(TEMP_DIR, 'gitleaks-test-config.toml');
const TEMP_REPORT_PATH = join(TEMP_DIR, 'gitleaks-test-report.json');

function writeTempTestConfig(): void {
  const config = [
    'title = "Test Gitleaks Config"',
    '',
    '[extend]',
    'useDefault = true',
    '',
    '[[rules]]',
    'id = "stellar-secret-key"',
    'description = "Stellar Secret Key (S + 55 base58 chars)"',
    'regex = "(?i)S[a-zA-Z0-9]{55}"',
  ].join('\n');
  writeFileSync(TEMP_CONFIG_PATH, config);
}

function runGitleaks(sourcePath: string): number {
  const cmd = [
    'gitleaks',
    'detect',
    '--no-git',
    '--no-banner',
    '--ignore-gitleaks-allow',
    '-s',
    sourcePath,
    '-f',
    'json',
    '-c',
    TEMP_CONFIG_PATH,
    '-r',
    TEMP_REPORT_PATH,
  ].join(' ');
  try {
    execSync(cmd, { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] });
    return 0;
  } catch (error: unknown) {
    const err = error as { status?: number };
    return err.status ?? 1;
  }
}

function cleanupTestFiles(): void {
  try {
    if (existsSync(TEMP_CONFIG_PATH)) unlinkSync(TEMP_CONFIG_PATH);
    if (existsSync(TEMP_REPORT_PATH)) unlinkSync(TEMP_REPORT_PATH);
  } catch {
    // ignore cleanup errors
  }
}

describe('Gitleaks Secret Scanning Verification', () => {
  beforeAll(() => {
    writeTempTestConfig();
  });

  afterAll(() => {
    cleanupTestFiles();
  });

  describe('Stellar secret key detection', () => {
    it('fixture file should contain Stellar secret key patterns', () => {
      const fixtureFile = join(FIXTURE_DIR, 'invalid-stellar-secret.env');
      expect(existsSync(fixtureFile)).toBe(true);

      const content = readFileSync(fixtureFile, 'utf-8');
      const secretRegex = /(?i)S[a-zA-Z0-9]{55}/;
      const matches = content.match(secretRegex);
      expect(matches).toBeTruthy();
      expect(matches!.length).toBeGreaterThanOrEqual(1);
    });

    it('gitleaks should detect Stellar secret keys in fixture files', () => {
      const exitCode = runGitleaks(FIXTURE_DIR);
      expect(exitCode).toBeGreaterThan(0);

      const report = readFileSync(TEMP_REPORT_PATH, 'utf-8');
      const results = JSON.parse(report);
      expect(Array.isArray(results)).toBe(true);
      expect(results.length).toBeGreaterThan(0);
    });

    it('main .gitleaks.toml should contain Stellar secret detector rule', () => {
      const configContent = readFileSync(GITLEAKS_CONFIG, 'utf-8');
      expect(configContent).toContain('Stellar Secret Key');
      expect(configContent).toContain('(?i)S[a-zA-Z0-9]{55}');
    });
  });

  describe('gitleaks local verification', () => {
    it('gitleaks should be installed and available', () => {
      let available = false;
      try {
        execSync('gitleaks version', { encoding: 'utf-8', stdio: 'pipe' });
        available = true;
      } catch {
        available = false;
      }
      expect(available).toBe(true);
    });
  });
});
