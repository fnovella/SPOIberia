# SPOIberia (`spoiberia`)

Aplicación multiplataforma escrita en **Rust** con **arquitectura dual** (**Interfaz TUI interactiva** + **CLI Headless para Agentes Autónomos**) para la gestión de autorizaciones, aprovisionamiento de bibliotecas/carpetas desde matrices externas y clonado completo de sitios en **SharePoint Online (Microsoft 365)** con persistencia transaccional y **Rollback en SQLite**.

---

## Características Principales

### Fase 1 — Autorizaciones de Grupos y Usuarios + Cola Staging + Rollback SQLite
- **Autenticación OAuth2 Nativa contra Microsoft Entra ID**:
  - Soporta **Interactive Browser (PKCE)** y **Device Code Flow** sin depender de binarios externos.
  - Intercambio automático de `refresh_token` multi-recurso para **Microsoft Graph** y **SharePoint Online REST API**.
- **Descubrimiento Híbrido de Sitios SharePoint (Admin / Owner)**:
  - Combina los sitios de grupos M365 propios (`/me/ownedObjects`) con búsqueda en SharePoint (`contentclass:STS_Site`) y verificación concurrente de permisos (`CurrentUser/IsSiteAdmin` y máscara `EffectiveBasePermissions` bit `ManagePermissions`).
- **Cola de Cambios (*Staging*) y Persistencia Transaccional SQLite (`rusqlite` bundled)**:
  - En la TUI, las altas (`[+]` en verde) y bajas (`[-]` en rojo) se acumulan en la **Cola de Staging** y solo se ejecutan al pulsar **`A` (Aplicar)**.
  - Cada lote aplicado registra una **Transacción** en SQLite (`spoiberia.db`) junto con el `reverse_payload` exacto de cada operación, permitiendo hacer **Rollback (Revertir)** de transacciones completas o de operaciones individuales en orden inverso (`LIFO`).

### Fase 2 — Bibliotecas, Carpetas, Permisos Únicos y Matriz Externa (CSV / JSON)
- Exploración y creación de **Bibliotecas de Documentos** y **Carpetas/Subcarpetas** (en *"Documentos"* / `Shared Documents` o bibliotecas adicionales).
- Ruptura de herencia de permisos (`BreakRoleInheritance`), **creación automática de grupos de SharePoint si no existen**, y asignación de niveles de permiso (`Read`, `Contribute`, `Edit`, `Full Control` independientes del idioma del sitio).
- **Importación y Exportación de Matriz Externa (`.csv` y `.json`)**:
  - Ordena automáticamente las carpetas *top-down* por profundidad de ruta para configurar carpetas padre antes que subcarpetas.

### Fase 3 — Clonado Completo de un Sitio SharePoint en otro
- Copia integral desde un sitio **Origen** a un sitio **Destino** en el mismo tenant:
  1. Sincronización de miembros en grupos asociados (`Owners`, `Members`, `Visitors`) y creación de grupos personalizados con sus miembros y roles de sitio.
  2. Creación de bibliotecas de documentos faltantes y **copia Server-Side (`SP.MoveCopyUtil`)** de todas las carpetas y archivos conservando su historial de versiones sin descargar datos por la máquina local.
  3. Replicación *top-down* de los **permisos únicos (herencia rota)** a nivel de carpeta hacia los grupos y usuarios equivalentes del destino.

### Fase 4 — Control de Sincronización Offline (OneDrive Sync / Accesos Directos)
- Activación y desactivación mediante SharePoint Online REST API de `ExcludeFromOfflineClient` tanto a nivel de **Sitio (`SP.Web`)** como de **Bibliotecas de Documentos (`SP.List`)**, con soporte recursivo de subsitios (`--include-subsites`).
- Oculta los botones **Sincronizar** y **Agregar acceso directo a OneDrive** e impide nuevas sincronizaciones o accesos directos hacia los OneDrive privativos de los usuarios, manteniendo auditoría transaccional y **Rollback en SQLite** del estado previo exacto.

---

## Uso Interactivo (TUI)

Ejecuta `spoiberia` sin argumentos (o `spoiberia tui`):

```bash
spoiberia
```

### Atajos de Teclado Principales en la TUI
- **Global**: `q` (Salir), `?` (Ayuda), `A` (**Aplicar cambios en Staging**), `1`/`2`/`3`/`4` (Cambiar entre pestañas del sitio), `O` (Activar/Desactivar sync offline en el sitio y todas sus bibliotecas).
- **Selector de Sitios**: `↑`/`↓` (Navegar), `Enter` (Abrir sitio), `/` (Filtrar local), `s` (Buscar en el tenant), `u` (Abrir URL manual), `c` (Fijar como sitio Origen para Clonado).
- **Pestaña 1 (Grupos y Usuarios)**: `Tab`/`←`/`→` (Cambiar panel), `a` (Buscar usuario en directorio M365 y asignar), `d` (Remover usuario), `g` (Crear grupo).
- **Pestaña 2 (Bibliotecas y Carpetas)**: `o` (Activar/Desactivar sync offline en biblioteca seleccionada), `O` (En sitio y todas sus bibliotecas), `n` (Nueva carpeta), `b` (Nueva biblioteca), `p` (Asignar permiso único a carpeta), `i` (Importar matriz CSV/JSON), `e` (Exportar matriz CSV/JSON).
- **Pestaña 3 (Clonar Sitio)**: `Espacio` (Marcar/desmarcar opciones), `s` (URL Sitio Origen), `Enter` (Generar plan de clonado en Staging).
- **Pestaña 4 (Staging e Historial SQLite)**: `A` (Aplicar cola Staging), `R` (**Revertir transacción completa**), `r` (**Revertir operación individual**), `d` (Descartar ítem de Staging), `X` (Vaciar Staging).

---

## Uso en Modo Comando (CLI para Agentes Autónomos y Scripts)

Todos los subcomandos soportan `--json` (o `-o json|table|csv`) y `--dry-run`:

```bash
# 1. Autenticación
spoiberia auth status --json
spoiberia auth login [--device-code]

# 2. Listar o verificar sitios donde eres Admin o Propietario
spoiberia sites list --json
spoiberia sites check --site "https://contoso.sharepoint.com/sites/MiSitio" --json

# 3. Grupos y miembros
spoiberia groups list --site "https://contoso.sharepoint.com/sites/MiSitio" --json
spoiberia groups members --site "https://contoso.sharepoint.com/sites/MiSitio" --group "MiSitio Members" --json
spoiberia users search --query "novella" --json

# 4. Asignar o remover usuarios (generando transacción reversible en SQLite)
spoiberia members add \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --group "MiSitio Members" \
  --users "usuario1@contoso.com,usuario2@contoso.com" \
  --note "Alta solicitada por agente" \
  --dry-run --json

# 5. Historial de transacciones y Rollback en SQLite
spoiberia tx list --json
spoiberia tx show 1 --json
spoiberia tx revert 1 --json

# 6. Fase 2: Bibliotecas, Carpetas y Matriz Externa (CSV / JSON)
spoiberia folders grant \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --library "Documentos" \
  --path "01_Direccion/Confidencial" \
  --group "SPO_Comite_Ejecutivo" \
  --role "Full Control" \
  --create-group \
  --dry-run --json

spoiberia matrix apply \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --file ./examples/sample-permissions.csv \
  --dry-run --json

spoiberia matrix export \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --library "Documentos" \
  --out ./permisos-exportados.json

# 7. Fase 3: Clonado Completo de un SharePoint en otro
spoiberia clone \
  --source-site "https://contoso.sharepoint.com/sites/PlantillaObra" \
  --target-site "https://contoso.sharepoint.com/sites/NuevaObra" \
  --dry-run --json

# 8. Fase 4: Control de Sincronización Offline (OneDrive Sync / Accesos Directos)
spoiberia sync status \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --include-subsites --json

spoiberia sync disable \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --include-subsites \
  --note "Bloquear sincronización en OneDrive" \
  --dry-run --json

spoiberia sync enable \
  --site "https://contoso.sharepoint.com/sites/MiSitio" \
  --library "Documentos" \
  --dry-run --json
```

---

## Compilación de Binarios Multiplataforma (Linux, Windows y macOS)

Ejecuta el script de construcción para generar los ejecutables autónomos en `./dist/`:

```bash
./scripts/build-release.sh
```

Artefactos generados en `dist/`:
- `dist/spoiberia-linux-x86_64` (Linux ELF 64-bit)
- `dist/spoiberia-windows-x86_64.exe` (Windows PE32+ 64-bit)
- `dist/spoiberia-macos-aarch64` (macOS Apple Silicon Mach-O 64-bit)
