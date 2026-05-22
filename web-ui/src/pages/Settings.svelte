<script lang="ts">
  import Card from "../components/Card.svelte";
  import { Construction, Network, Clock, MessagesSquare, Lock, FileSearch } from "lucide-svelte";
</script>

<div class="space-y-5">
  <Card title="Управление модулем"
        subtitle="Базовые настройки сетевого окружения и безопасности">
    <div class="flex items-center gap-3 rounded-md border border-amber-500/30
                bg-amber-500/5 p-4 text-sm text-amber-200">
      <Construction size={18} class="shrink-0" />
      <div>
        Эта страница — заготовка. Конкретные блоки появятся, когда у agent
        будет соответствующий REST API и cryptod начнёт сохранять конфигурацию
        в SQLite.
      </div>
    </div>
  </Card>

  <div class="grid grid-cols-1 gap-5 md:grid-cols-2">
    <Card title="Сетевые настройки" subtitle="Управление VLAN / IP / маршрутизация">
      <ul class="space-y-2.5 text-sm text-slate-300">
        <li class="flex items-start gap-2">
          <Network size={14} class="mt-0.5 text-slate-500" />
          Management VLAN ID, IP/маска, шлюз, DNS
        </li>
        <li class="flex items-start gap-2">
          <Network size={14} class="mt-0.5 text-slate-500" />
          Auto-negotiation и скорость портов A/B
        </li>
        <li class="flex items-start gap-2">
          <Network size={14} class="mt-0.5 text-slate-500" />
          Bridge ↔ L3-routing режим работы
        </li>
      </ul>
    </Card>

    <Card title="Время и синхронизация" subtitle="NTP / PTP">
      <ul class="space-y-2.5 text-sm text-slate-300">
        <li class="flex items-start gap-2">
          <Clock size={14} class="mt-0.5 text-slate-500" />
          NTP-серверы заказчика
        </li>
        <li class="flex items-start gap-2">
          <Clock size={14} class="mt-0.5 text-slate-500" />
          PTP-режим (Boundary Clock / Ordinary Clock)
        </li>
        <li class="flex items-start gap-2">
          <Clock size={14} class="mt-0.5 text-slate-500" />
          Часовой пояс
        </li>
      </ul>
    </Card>

    <Card title="Журналирование" subtitle="Syslog forwarder, audit log">
      <ul class="space-y-2.5 text-sm text-slate-300">
        <li class="flex items-start gap-2">
          <MessagesSquare size={14} class="mt-0.5 text-slate-500" />
          Адреса SIEM / Loki (TLS)
        </li>
        <li class="flex items-start gap-2">
          <FileSearch size={14} class="mt-0.5 text-slate-500" />
          Глубина локального аудита
        </li>
      </ul>
    </Card>

    <Card title="Безопасность" subtitle="Управление учётками, SSH-ключи, mTLS">
      <ul class="space-y-2.5 text-sm text-slate-300">
        <li class="flex items-start gap-2">
          <Lock size={14} class="mt-0.5 text-slate-500" />
          Смена паролей user / root
        </li>
        <li class="flex items-start gap-2">
          <Lock size={14} class="mt-0.5 text-slate-500" />
          SSH authorized_keys
        </li>
        <li class="flex items-start gap-2">
          <Lock size={14} class="mt-0.5 text-slate-500" />
          TLS-сертификат web-UI и API
        </li>
      </ul>
    </Card>
  </div>
</div>
