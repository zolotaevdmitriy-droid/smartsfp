<script lang="ts">
  import Card from "../components/Card.svelte";
  import { Construction } from "lucide-svelte";
</script>

<Card title="Журнал событий"
      subtitle="Live-tail из syslog/journald и аудит-события cryptod">
  <div class="flex items-center gap-3 rounded-md border border-amber-500/30
              bg-amber-500/5 p-4 text-sm text-amber-200">
    <Construction size={18} class="shrink-0" />
    <div>
      Эта страница появится после реализации SSE-эндпоинта в agent
      и audit-log в SQLite. Пока что логи доступны на модуле в
      <code class="font-mono text-amber-100">/tmp/acm/agent.log</code>
      и
      <code class="font-mono text-amber-100">/tmp/acm/cryptod.log</code>.
    </div>
  </div>
</Card>
