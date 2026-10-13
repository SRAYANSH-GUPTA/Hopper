/** @vitest-environment jsdom */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sentryInitMock = vi.fn();
const sentryMetricsCountMock = vi.fn();
const renderMock = vi.fn();
const createRootMock = vi.fn(() => ({
  render: renderMock,
}));

vi.mock("@sentry/react", () => ({
  init: sentryInitMock,
  metrics: {
    count: sentryMetricsCountMock,
  },
}));

vi.mock("react-dom/client", () => ({
  default: {
    createRoot: createRootMock,
  },
  createRoot: createRootMock,
}));

vi.mock("./App", () => ({
  default: () => null,
}));

describe("main sentry bootstrap", () => {
  beforeEach(() => {
    vi.resetModules();
    sentryInitMock.mockClear();
    sentryMetricsCountMock.mockClear();
    createRootMock.mockClear();
    renderMock.mockClear();
    document.body.innerHTML = '<div id="root"></div>';
    vi.stubEnv("VITE_SENTRY_DSN", "");
  });

  afterEach(() => vi.unstubAllEnvs());

  it.each(["", "   "])("starts without telemetry for an unconfigured DSN (%j)", async (dsn) => {
    vi.stubEnv("VITE_SENTRY_DSN", dsn);
    await import("./main");
    expect(sentryInitMock).not.toHaveBeenCalled();
    expect(sentryMetricsCountMock).not.toHaveBeenCalled();
    expect(renderMock).toHaveBeenCalledTimes(1);
  });

  it("uses only the explicitly configured telemetry destination", async () => {
    vi.stubEnv("VITE_SENTRY_DSN", " https://public@example.com/42 ");
    await import("./main");

    expect(sentryInitMock).toHaveBeenCalledTimes(1);
    expect(sentryInitMock).toHaveBeenCalledWith(
      expect.objectContaining({
        dsn: "https://public@example.com/42",
        sendDefaultPii: false,
        release: expect.any(String),
      }),
    );
    expect(sentryMetricsCountMock).toHaveBeenCalledTimes(1);
    expect(sentryMetricsCountMock).toHaveBeenCalledWith(
      "app_open",
      1,
      expect.objectContaining({
        attributes: expect.objectContaining({
          env: "test",
        }),
      }),
    );
  });
});
