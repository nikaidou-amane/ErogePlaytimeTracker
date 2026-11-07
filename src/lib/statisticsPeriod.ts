/**
 * 統計画面の月・年の選択を、アプリを動かしたまま画面を移動しても保つための保管場所。
 *
 * 画面を離れるとコンポーネントは破棄されるため、選択はコンポーネントの外に置く。
 * ここはメモリだけなので、アプリを再起動すると初期化され、月・年は現在の年月へ戻る
 * （要件: 再起動時だけリセットし、ソフト内の画面移動では保つ）。
 */
export interface RememberedPeriod {
  year: number;
  month: number;
}

let remembered: RememberedPeriod | null = null;

export function loadRememberedPeriod(): RememberedPeriod | null {
  return remembered;
}

export function saveRememberedPeriod(period: RememberedPeriod) {
  remembered = { ...period };
}

/** test 用。実際の画面移動では呼ばない。 */
export function clearRememberedPeriod() {
  remembered = null;
}
