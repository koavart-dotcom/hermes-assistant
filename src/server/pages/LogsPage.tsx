import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { api } from "../api";
import { ErrorText, Pre, Section, Select } from "../ui";

export function LogsPage() {
  const [tail, setTail] = useState("300");
  const logs = useQuery({
    queryKey: ["logs", tail],
    queryFn: () => api.logs(Number(tail)),
    refetchInterval: 10_000,
  });
  return (
    <>
      <h1 className="text-xl font-semibold">Журнал службы</h1>
      <Section
        title="hermesd.log"
        description="Время в UTC. Обновляется раз в 10 секунд."
        actions={
          <>
            <Select
              value={tail}
              onChange={setTail}
              options={["100", "300", "1000"].map((v) => ({ value: v, label: `${v} строк` }))}
            />
            <Button size="sm" variant="outline" onClick={() => void logs.refetch()}>
              Обновить
            </Button>
          </>
        }
      >
        <ErrorText error={logs.error} />
        <Pre className="max-h-[70vh]">{logs.data ?? "…"}</Pre>
      </Section>
    </>
  );
}
