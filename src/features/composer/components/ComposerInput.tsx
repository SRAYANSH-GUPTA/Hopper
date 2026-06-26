import { useCallback, useRef } from "react";
import type {
  ChangeEvent,
  ClipboardEvent,
  KeyboardEvent,
  RefObject,
  SyntheticEvent,
} from "react";
import type { AutocompleteItem } from "../hooks/useComposerAutocomplete";
import ChevronDown from "lucide-react/dist/esm/icons/chevron-down";
import ChevronUp from "lucide-react/dist/esm/icons/chevron-up";
import Mic from "lucide-react/dist/esm/icons/mic";
import Square from "lucide-react/dist/esm/icons/square";
import X from "lucide-react/dist/esm/icons/x";
import { useComposerImageDrop } from "../hooks/useComposerImageDrop";
import { ComposerMobileActionsMenu } from "./ComposerMobileActionsMenu";
import { ComposerSuggestionsPopover } from "./ComposerSuggestionsPopover";
import { ComposerAttachments } from "./ComposerAttachments";
import { DictationWaveform } from "../../dictation/components/DictationWaveform";
import { useComposerDictationControls } from "../hooks/useComposerDictationControls";
import { useComposerInputLayout } from "../hooks/useComposerInputLayout";
import { useComposerMobileActions } from "../hooks/useComposerMobileActions";
import type { ReviewPromptState, ReviewPromptStep } from "../../threads/hooks/useReviewPrompt";

const IMAGE_PATH_EXTENSIONS = [
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".bmp",
  ".tiff",
  ".tif",
  ".heic",
  ".heif",
];

function isImagePath(value: string) {
  const lower = value.toLowerCase();
  return IMAGE_PATH_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

function isPastedImagePath(value: string) {
  const trimmed = value.trim().replace(/^["']|["']$/g, "");
  if (!trimmed || /[\r\n]/.test(trimmed)) {
    return false;
  }
  const isAbsoluteUnixPath = trimmed.startsWith("/");
  const isAbsoluteWindowsPath = /^[A-Za-z]:[\\/]/.test(trimmed);
  return (isAbsoluteUnixPath || isAbsoluteWindowsPath) && isImagePath(trimmed);
}

function normalizeClipboardPath(value: string) {
  const trimmed = value.trim().replace(/^["']|["']$/g, "");
  if (!trimmed) {
    return "";
  }
  if (trimmed.startsWith("file://")) {
    try {
      return decodeURI(trimmed.replace(/^file:\/\//, ""));
    } catch {
      return trimmed.replace(/^file:\/\//, "");
    }
  }
  return trimmed;
}

function extractPastedImagePath(value: string) {
  const normalized = normalizeClipboardPath(value);
  if (isPastedImagePath(normalized)) {
    return normalized;
  }

  for (const line of normalized.split(/\r?\n/)) {
    const candidate = normalizeClipboardPath(line);
    if (isPastedImagePath(candidate)) {
      return candidate;
    }
  }

  const matches = normalized.match(
    /(?:file:\/\/)?(?:[A-Za-z]:[\\/]|\/)[^\r\n]+?\.(?:png|jpe?g|gif|webp|bmp|tiff?|heic|heif)\b/gi,
  );
  return matches?.map(normalizeClipboardPath).find(isPastedImagePath) ?? "";
}

type ComposerInputProps = {
  text: string;
  disabled: boolean;
  sendLabel: string;
  canStop: boolean;
  canSend: boolean;
  isProcessing: boolean;
  onStop: () => void;
  onSend: () => void;
  dictationState?: "idle" | "listening" | "processing";
  dictationLevel?: number;
  dictationEnabled?: boolean;
  onToggleDictation?: () => void;
  onCancelDictation?: () => void;
  onOpenDictationSettings?: () => void;
  dictationError?: string | null;
  onDismissDictationError?: () => void;
  dictationHint?: string | null;
  onDismissDictationHint?: () => void;
  attachments?: string[];
  onAddAttachment?: () => void;
  onAttachImages?: (paths: string[]) => void;
  onRemoveAttachment?: (path: string) => void;
  onTextChange: (next: string, selectionStart: number | null) => void;
  onTextPaste?: (event: ClipboardEvent<HTMLTextAreaElement>) => void;
  onSelectionChange: (selectionStart: number | null) => void;
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
  isExpanded?: boolean;
  onToggleExpand?: () => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  suggestionsOpen: boolean;
  suggestions: AutocompleteItem[];
  highlightIndex: number;
  onHighlightIndex: (index: number) => void;
  onSelectSuggestion: (item: AutocompleteItem) => void;
  suggestionsStyle?: React.CSSProperties;
  reviewPrompt?: ReviewPromptState;
  onReviewPromptClose?: () => void;
  onReviewPromptShowPreset?: () => void;
  onReviewPromptChoosePreset?: (
    preset: Exclude<ReviewPromptStep, "preset"> | "uncommitted",
  ) => void;
  highlightedPresetIndex?: number;
  onReviewPromptHighlightPreset?: (index: number) => void;
  highlightedBranchIndex?: number;
  onReviewPromptHighlightBranch?: (index: number) => void;
  highlightedCommitIndex?: number;
  onReviewPromptHighlightCommit?: (index: number) => void;
  onReviewPromptSelectBranch?: (value: string) => void;
  onReviewPromptSelectBranchAtIndex?: (index: number) => void;
  onReviewPromptConfirmBranch?: () => Promise<void>;
  onReviewPromptSelectCommit?: (sha: string, title: string) => void;
  onReviewPromptSelectCommitAtIndex?: (index: number) => void;
  onReviewPromptConfirmCommit?: () => Promise<void>;
  onReviewPromptUpdateCustomInstructions?: (value: string) => void;
  onReviewPromptConfirmCustom?: () => Promise<void>;
  hideSendButton?: boolean;
};

export function ComposerInput({
  text,
  disabled,
  sendLabel,
  canStop,
  canSend,
  isProcessing,
  onStop,
  onSend,
  dictationState = "idle",
  dictationLevel = 0,
  dictationEnabled = false,
  onToggleDictation,
  onCancelDictation,
  onOpenDictationSettings,
  dictationError = null,
  onDismissDictationError,
  dictationHint = null,
  onDismissDictationHint,
  attachments = [],
  onAddAttachment,
  onAttachImages,
  onRemoveAttachment,
  onTextChange,
  onTextPaste,
  onSelectionChange,
  onKeyDown,
  isExpanded = false,
  onToggleExpand,
  textareaRef,
  suggestionsOpen,
  suggestions,
  highlightIndex,
  onHighlightIndex,
  onSelectSuggestion,
  suggestionsStyle,
  reviewPrompt,
  onReviewPromptClose,
  onReviewPromptShowPreset,
  onReviewPromptChoosePreset,
  highlightedPresetIndex,
  onReviewPromptHighlightPreset,
  highlightedBranchIndex,
  onReviewPromptHighlightBranch,
  highlightedCommitIndex,
  onReviewPromptHighlightCommit,
  onReviewPromptSelectBranch,
  onReviewPromptSelectBranchAtIndex,
  onReviewPromptConfirmBranch,
  onReviewPromptSelectCommit,
  onReviewPromptSelectCommitAtIndex,
  onReviewPromptConfirmCommit,
  onReviewPromptUpdateCustomInstructions,
  onReviewPromptConfirmCustom,
  hideSendButton = false,
}: ComposerInputProps) {
  const suggestionListRef = useRef<HTMLDivElement | null>(null);
  const suggestionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const { isPhoneLayout, isPhoneTallInput } = useComposerInputLayout({
    isExpanded,
    text,
    textareaRef,
  });
  const { mobileActionsOpen, mobileActionsRef, setMobileActionsOpen } =
    useComposerMobileActions({ disabled });
  const {
    dropTargetRef,
    isDragOver,
    handleDragOver,
    handleDragEnter,
    handleDragLeave,
    handleDrop,
    handlePaste,
  } = useComposerImageDrop({
    disabled,
    onAttachImages,
  });
  const handleActionClick = useCallback(() => {
    if (canStop) {
      onStop();
      return;
    }
    onSend();
  }, [canStop, onSend, onStop]);
  const {
    handleMicClick,
    isDictating,
    isDictationBusy,
    isDictationProcessing,
    micAriaLabel,
    micDisabled,
    micTitle,
  } = useComposerDictationControls({
    disabled,
    dictationEnabled,
    dictationState,
    onToggleDictation,
    onCancelDictation,
    onOpenDictationSettings,
  });

  const handleTextareaChange = useCallback(
    (event: ChangeEvent<HTMLTextAreaElement>) => {
      onTextChange(event.target.value, event.target.selectionStart);
    },
    [onTextChange],
  );

  const handleTextareaSelect = useCallback(
    (event: SyntheticEvent<HTMLTextAreaElement>) => {
      onSelectionChange((event.target as HTMLTextAreaElement).selectionStart);
    },
    [onSelectionChange],
  );

  const handleTextareaPaste = useCallback(
    (event: ClipboardEvent<HTMLTextAreaElement>) => {
      let pastedText = "";
      try {
        const cd = event.clipboardData;
        if (cd) {
          pastedText =
            cd.getData("text/plain") ||
            cd.getData("text") ||
            cd.getData("text/uri-list") ||
            "";
        }
      } catch {
        // clipboardData may not be accessible in some WebView environments
      }
      const pastedPath = extractPastedImagePath(pastedText);
      if (pastedPath && isPastedImagePath(pastedPath)) {
        event.preventDefault();
        onAttachImages?.([pastedPath]);
        return;
      }
      void handlePaste(event);
      if (!event.defaultPrevented) {
        onTextPaste?.(event);
      }

      // Fallback: if clipboardData was unavailable, check the textarea
      // after the browser inserts the pasted text
      if (!pastedText) {
        const textarea = event.currentTarget;
        const valueBefore = textarea.value;
        const selBefore = textarea.selectionStart;
        requestAnimationFrame(() => {
          const valueAfter = textarea.value;
          if (valueAfter === valueBefore) return;
          const inserted = selBefore != null
            ? valueAfter.slice(selBefore, selBefore + (valueAfter.length - valueBefore.length))
            : valueAfter.slice(valueBefore.length);
          const fallbackPath = extractPastedImagePath(inserted);
          if (fallbackPath && isPastedImagePath(fallbackPath)) {
            const restored = valueBefore;
            onTextChange(restored, selBefore);
            onAttachImages?.([fallbackPath]);
          }
        });
      }
    },
    [handlePaste, onAttachImages, onTextPaste, onTextChange],
  );

  const handleMobileAttachClick = useCallback(() => {
    if (disabled || !onAddAttachment) {
      return;
    }
    setMobileActionsOpen(false);
    onAddAttachment();
  }, [disabled, onAddAttachment]);

  const handleMobileExpandClick = useCallback(() => {
    if (disabled || !onToggleExpand) {
      return;
    }
    setMobileActionsOpen(false);
    onToggleExpand();
  }, [disabled, onToggleExpand]);

  const handleMobileDictationClick = useCallback(() => {
    setMobileActionsOpen(false);
    handleMicClick();
  }, [handleMicClick]);

  return (
    <div className={`composer-input${isPhoneLayout && isPhoneTallInput ? " is-phone-tall" : ""}`}>
      <div
        className={`composer-input-area${isDragOver ? " is-drag-over" : ""}`}
        ref={dropTargetRef}
        onDragOver={handleDragOver}
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <ComposerAttachments
          attachments={attachments}
          disabled={disabled}
          onRemoveAttachment={onRemoveAttachment}
        />
        <div className="composer-input-row">
          <ComposerMobileActionsMenu
            disabled={disabled}
            handleMobileAttachClick={handleMobileAttachClick}
            handleMobileDictationClick={handleMobileDictationClick}
            handleMobileExpandClick={handleMobileExpandClick}
            isDictating={isDictating}
            isDictationProcessing={isDictationProcessing}
            isExpanded={isExpanded}
            micAriaLabel={micAriaLabel}
            micDisabled={micDisabled}
            mobileActionsOpen={mobileActionsOpen}
            mobileActionsRef={mobileActionsRef}
            onAddAttachment={onAddAttachment}
            onToggleExpand={onToggleExpand}
            setMobileActionsOpen={setMobileActionsOpen}
            showDictationAction={Boolean(
              onToggleDictation || onOpenDictationSettings || onCancelDictation,
            )}
          />
          <textarea
            ref={textareaRef}
            placeholder={
              disabled
                ? "Review in progress. Chat will re-enable when it completes."
                : "Ask for follow-up changes or attach images"
            }
            value={text}
            onChange={handleTextareaChange}
            onSelect={handleTextareaSelect}
            disabled={disabled}
            onKeyDown={onKeyDown}
            onDragOver={handleDragOver}
            onDragEnter={handleDragEnter}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            onPaste={handleTextareaPaste}
          />
          <div className="composer-input-actions">
            {onToggleExpand && (
              <button
                className={`composer-action composer-action--expand${
                  isExpanded ? " is-active" : ""
                }`}
                onClick={onToggleExpand}
                disabled={disabled}
                aria-label={isExpanded ? "Collapse input" : "Expand input"}
                title={isExpanded ? "Collapse input" : "Expand input"}
              >
                {isExpanded ? <ChevronDown aria-hidden /> : <ChevronUp aria-hidden />}
              </button>
            )}
            <button
              className={`composer-action composer-action--mic${
                isDictationBusy ? " is-active" : ""
              }${isDictationProcessing ? " is-processing is-stop" : ""}${
                micDisabled ? " is-disabled" : ""
              }`}
              onClick={handleMicClick}
              disabled={micDisabled}
              aria-label={micAriaLabel}
              title={micTitle}
            >
              {isDictationProcessing ? (
                <X aria-hidden />
              ) : isDictating ? (
                <Square aria-hidden />
              ) : (
                <Mic aria-hidden />
              )}
            </button>
            {!hideSendButton && (
              <button
                className={`composer-action${canStop ? " is-stop" : " is-send"}${
                  canStop && isProcessing ? " is-loading" : ""
                }`}
                onClick={handleActionClick}
                disabled={(disabled && !canStop) || isDictationBusy || (!canStop && !canSend)}
                aria-label={canStop ? "Stop" : sendLabel}
                title={canStop ? "Stop" : sendLabel}
              >
                {canStop ? (
                  <>
                    <span className="composer-action-stop-square" aria-hidden />
                    {isProcessing && (
                      <span className="composer-action-spinner" aria-hidden />
                    )}
                  </>
                ) : (
                  <svg viewBox="0 0 24 24" fill="none" aria-hidden>
                    <path
                      d="M12 5l6 6m-6-6L6 11m6-6v14"
                      stroke="currentColor"
                      strokeWidth="1.7"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                )}
              </button>
            )}
          </div>
        </div>
        {isDictationBusy && (
          <DictationWaveform
            active={isDictating}
            processing={dictationState === "processing"}
            level={dictationLevel}
          />
        )}
        {dictationError && (
          <div className="composer-dictation-error" role="status">
            <span>{dictationError}</span>
            <button
              type="button"
              className="ghost composer-dictation-error-dismiss"
              onClick={onDismissDictationError}
            >
              Dismiss
            </button>
          </div>
        )}
        {dictationHint && (
          <div className="composer-dictation-hint" role="status">
            <span>{dictationHint}</span>
            {onDismissDictationHint && (
              <button
                type="button"
                className="ghost composer-dictation-error-dismiss"
                onClick={onDismissDictationHint}
              >
                Dismiss
              </button>
            )}
          </div>
        )}
        <ComposerSuggestionsPopover
          highlightIndex={highlightIndex}
          highlightedBranchIndex={highlightedBranchIndex}
          highlightedCommitIndex={highlightedCommitIndex}
          highlightedPresetIndex={highlightedPresetIndex}
          onHighlightIndex={onHighlightIndex}
          onReviewPromptChoosePreset={onReviewPromptChoosePreset}
          onReviewPromptClose={onReviewPromptClose}
          onReviewPromptConfirmBranch={onReviewPromptConfirmBranch}
          onReviewPromptConfirmCommit={onReviewPromptConfirmCommit}
          onReviewPromptConfirmCustom={onReviewPromptConfirmCustom}
          onReviewPromptHighlightBranch={onReviewPromptHighlightBranch}
          onReviewPromptHighlightCommit={onReviewPromptHighlightCommit}
          onReviewPromptHighlightPreset={onReviewPromptHighlightPreset}
          onReviewPromptSelectBranch={onReviewPromptSelectBranch}
          onReviewPromptSelectBranchAtIndex={onReviewPromptSelectBranchAtIndex}
          onReviewPromptSelectCommit={onReviewPromptSelectCommit}
          onReviewPromptSelectCommitAtIndex={onReviewPromptSelectCommitAtIndex}
          onReviewPromptShowPreset={onReviewPromptShowPreset}
          onReviewPromptUpdateCustomInstructions={onReviewPromptUpdateCustomInstructions}
          onSelectSuggestion={onSelectSuggestion}
          reviewPrompt={reviewPrompt}
          suggestionListRef={suggestionListRef}
          suggestionRefs={suggestionRefs}
          suggestions={suggestions}
          suggestionsOpen={suggestionsOpen}
          suggestionsStyle={suggestionsStyle}
        />
      </div>
    </div>
  );
}
