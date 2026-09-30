import { useEffect, useState } from "react";
import { useTaskReminders } from "@/features/tasks/hooks/useTaskReminders";

function millisecondsUntilNextMinute(now: Date): number {
  return (60 - now.getSeconds()) * 1000 - now.getMilliseconds() + 20;
}

export function ClockStatus() {
  const [now, setNow] = useState(() => new Date());
  useTaskReminders();

  useEffect(() => {
    let timeout = 0;
    const schedule = () => {
      timeout = window.setTimeout(() => { setNow(new Date()); schedule(); }, millisecondsUntilNextMinute(new Date()));
    };
    schedule();
    return () => window.clearTimeout(timeout);
  }, []);

  const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(now);
  const weekday = new Intl.DateTimeFormat(undefined, { weekday: "short" }).format(now);

  return <div className="clock-status" aria-label="Current time"><span>{weekday}</span><time dateTime={now.toISOString()}>{time}</time></div>;
}
