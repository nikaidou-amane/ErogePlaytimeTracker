import { describe, expect, it, beforeEach } from 'vitest';
import {
  clearRememberedPeriod,
  loadRememberedPeriod,
  saveRememberedPeriod,
} from './statisticsPeriod';

describe('remembered statistics period', () => {
  beforeEach(() => clearRememberedPeriod());

  it('has nothing remembered before a period is chosen', () => {
    expect(loadRememberedPeriod()).toBeNull();
  });

  it('keeps the chosen month across page switches', () => {
    saveRememberedPeriod({ year: 2025, month: 11 });
    // 画面を離れて戻っても、同じ値が読める（モジュール変数なので破棄されない）。
    expect(loadRememberedPeriod()).toEqual({ year: 2025, month: 11 });
  });

  it('replaces the previous choice with the latest one', () => {
    saveRememberedPeriod({ year: 2025, month: 11 });
    saveRememberedPeriod({ year: 2024, month: 3 });
    expect(loadRememberedPeriod()).toEqual({ year: 2024, month: 3 });
  });

  it('does not alias the caller object', () => {
    const chosen = { year: 2025, month: 7 };
    saveRememberedPeriod(chosen);
    chosen.month = 1;
    expect(loadRememberedPeriod()).toEqual({ year: 2025, month: 7 });
  });
});
