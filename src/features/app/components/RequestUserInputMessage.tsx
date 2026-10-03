import { useEffect, useMemo, useState } from "react";
import type {
  RequestUserInputRequest,
  RequestUserInputResponse,
} from "../../../types";

type RequestUserInputMessageProps = {
  requests: RequestUserInputRequest[];
  activeThreadId: string | null;
  activeWorkspaceId?: string | null;
  onSubmit: (
    request: RequestUserInputRequest,
    response: RequestUserInputResponse,
  ) => void;
};

type SelectionState = Record<string, number[]>;
type NotesState = Record<string, string>;

export function RequestUserInputMessage({
  requests,
  activeThreadId,
  activeWorkspaceId,
  onSubmit,
}: RequestUserInputMessageProps) {
  const activeRequests = useMemo(
    () =>
      requests.filter((request) => {
        if (!activeThreadId) {
          return false;
        }
        if (request.params.thread_id !== activeThreadId) {
          return false;
        }
        if (activeWorkspaceId && request.workspace_id !== activeWorkspaceId) {
          return false;
        }
        return true;
      }),
    [requests, activeThreadId, activeWorkspaceId],
  );
  const activeRequest = activeRequests[0];
  const [selections, setSelections] = useState<SelectionState>({});
  const [notes, setNotes] = useState<NotesState>({});

  useEffect(() => {
    if (!activeRequest) {
      setSelections({});
      setNotes({});
      return;
    }
    const nextSelections: SelectionState = {};
    const nextNotes: NotesState = {};
    activeRequest.params.questions.forEach((question, index) => {
      const key = question.id || `question-${index}`;
      nextSelections[key] = [];
      nextNotes[key] = "";
    });
    setSelections(nextSelections);
    setNotes(nextNotes);
  }, [activeRequest]);

  if (!activeRequest) {
    return null;
  }

  const { questions } = activeRequest.params;
  const totalRequests = activeRequests.length;
  const answersAsFollowUp = activeRequest.params.answer_mode === "followUp";

  const buildAnswers = () => {
    const answers: RequestUserInputResponse["answers"] = {};
    questions.forEach((question, index) => {
      if (!question.id) {
        return;
      }
      const answerList: string[] = [];
      const key = question.id || `question-${index}`;
      const selectedIndexes = selections[key] ?? [];
      const options = question.options ?? [];
      const hasOptions = options.length > 0;
      if (hasOptions) {
        selectedIndexes.forEach((selectedIndex) => {
          const selected = options[selectedIndex];
          const selectedValue =
            selected?.label?.trim() || selected?.description?.trim() || "";
          if (selectedValue) {
            answerList.push(selectedValue);
          }
        });
      }
      const note = (notes[key] ?? "").trim();
      if (note) {
        if (hasOptions) {
          answerList.push(`user_note: ${note}`);
        } else {
          answerList.push(note);
        }
      }
      answers[question.id] = { answers: answerList };
    });
    return answers;
  };

  const handleSelect = (questionId: string, optionIndex: number, multiSelect: boolean) => {
    setSelections((current) => {
      const selected = current[questionId] ?? [];
      if (!multiSelect) {
        return { ...current, [questionId]: [optionIndex] };
      }
      const next = selected.includes(optionIndex)
        ? selected.filter((index) => index !== optionIndex)
        : [...selected, optionIndex].sort((a, b) => a - b);
      return { ...current, [questionId]: next };
    });
  };

  const handleNotesChange = (questionId: string, value: string) => {
    setNotes((current) => ({ ...current, [questionId]: value }));
  };

  const handleSubmit = () => {
    onSubmit(activeRequest, { answers: buildAnswers() });
  };

  return (
    <div className="message request-user-input-message">
      <div
        className="bubble request-user-input-card"
        role="group"
        aria-label="User input requested"
      >
        <div className="request-user-input-header">
          <div className="request-user-input-title">Input requested</div>
          {totalRequests > 1 ? (
            <div className="request-user-input-queue">
              {`Request 1 of ${totalRequests}`}
            </div>
          ) : null}
        </div>
        <div className="request-user-input-body">
          {questions.length ? (
            questions.map((question, index) => {
              const questionId = question.id || `question-${index}`;
              const selectedIndexes = selections[questionId] ?? [];
              const multiSelect = Boolean(question.multiSelect);
              const options = question.options ?? [];
              const notePlaceholder = question.isOther
                ? "Type your answer (optional)"
                : options.length
                ? "Add notes (optional)"
                : "Type your answer (optional)";
              return (
                <section key={questionId} className="request-user-input-question">
                  {question.header ? (
                    <div className="request-user-input-question-header">
                      {question.header}
                    </div>
                  ) : null}
                  <div className="request-user-input-question-text">
                    {question.question}
                  </div>
                  {options.length && multiSelect ? (
                    <div className="request-user-input-question-hint">Select all that apply</div>
                  ) : null}
                  {options.length ? (
                    <div
                      className="request-user-input-options"
                      role={multiSelect ? "group" : "radiogroup"}
                    >
                      {options.map((option, optionIndex) => (
                        <button
                          key={`${questionId}-${optionIndex}`}
                          type="button"
                          className={`request-user-input-option${
                            selectedIndexes.includes(optionIndex) ? " is-selected" : ""
                          }`}
                          role={multiSelect ? "checkbox" : "radio"}
                          aria-checked={selectedIndexes.includes(optionIndex)}
                          onClick={() => handleSelect(questionId, optionIndex, multiSelect)}
                        >
                          <div className="request-user-input-option-label">
                            {option.label}
                          </div>
                          {option.description ? (
                            <div className="request-user-input-option-description">
                              {option.description}
                            </div>
                          ) : null}
                        </button>
                      ))}
                    </div>
                  ) : null}
                  <textarea
                    className="request-user-input-notes"
                    placeholder={notePlaceholder}
                    value={notes[questionId] ?? ""}
                    onChange={(event) =>
                      handleNotesChange(questionId, event.target.value)
                    }
                    rows={2}
                  />
                </section>
              );
            })
          ) : (
            <div className="request-user-input-empty">
              No questions provided.
            </div>
          )}
        </div>
        {answersAsFollowUp ? (
          <div className="request-user-input-followup-hint">
            This agent can't pause for answers, so your answer is sent as your next message.
          </div>
        ) : null}
        <div className="request-user-input-actions">
          <button className="primary" onClick={handleSubmit}>
            {answersAsFollowUp ? "Send answer" : "Submit"}
          </button>
        </div>
      </div>
    </div>
  );
}
