import { useCallback } from "react";
import type { Dispatch, MutableRefObject } from "react";
import type { AccessMode, ApprovalRequest } from "@/types";
import {
  getApprovalCommandInfo,
  matchesCommandPrefix,
} from "@utils/approvalRules";
import { respondToServerRequest } from "@services/tauri";
import type { ThreadAction } from "./useThreadsReducer";

type UseThreadApprovalEventsOptions = {
  dispatch: Dispatch<ThreadAction>;
  approvalAllowlistRef: MutableRefObject<Record<string, string[][]>>;
  accessMode?: AccessMode | null;
};

export function useThreadApprovalEvents({
  dispatch,
  approvalAllowlistRef,
  accessMode,
}: UseThreadApprovalEventsOptions) {
  return useCallback(
    (approval: ApprovalRequest) => {
      // Full-access mode: silently approve everything without showing the UI.
      if (accessMode === "full-access") {
        void respondToServerRequest(
          approval.workspace_id,
          approval.request_id,
          "accept",
        );
        return;
      }

      const commandInfo = getApprovalCommandInfo(approval.params ?? {});
      const allowlist =
        approvalAllowlistRef.current[approval.workspace_id] ?? [];
      if (commandInfo && matchesCommandPrefix(commandInfo.tokens, allowlist)) {
        void respondToServerRequest(
          approval.workspace_id,
          approval.request_id,
          "accept",
        );
        return;
      }
      dispatch({ type: "addApproval", approval });
    },
    [accessMode, approvalAllowlistRef, dispatch],
  );
}
