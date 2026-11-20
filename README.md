# Centro LED

App nativa de Windows (Rust + Win32, ~310 KB) para controlar las luces y ver la batería de tus periféricos. En segundo plano usa unos pocos MB de RAM y prácticamente 0 % de CPU.

## Dispositivos

| Dispositivo | Batería | Luces |
|---|---|---|
| Teclado Tempest Diablo (0C45:8006) | — (con cable) | Teclas: 19 efectos o color libre tecla a tecla. Borde: 6 efectos propios con su color. Brillo, velocidad, arcoíris. Se guarda en el teclado |
| Mando DualSense / DualSense Edge (USB y Bluetooth) | Sí | Barra de luz: estático, respiración, **ciclo suave** entre 2–8 colores, arcoíris o apagada. LEDs de jugador (5, con brillo) y LED del micrófono (apagado/encendido/parpadeo) |
| Mando DualShock 4 | Sí | Barra de luz (mismos efectos) |
| Logitech HID++ (G733 y otros cascos/ratones/teclados) | Sí | Cada zona de luz por separado (p. ej. las dos luces del G733) con los efectos que admita el dispositivo: estático, respiración, pulso, ciclo de color, ondas… |
| Ratones VXE / ATK (receptor 2.4 GHz) | Sí, si el ratón está despierto (si duerme se muestra el último valor) | — |
| Mandos Xbox (XInput) | Aproximada | — |
| Cualquier dispositivo Bluetooth que informe batería a Windows | Sí | — |

Las luces de mandos y cascos se pierden al desconectarlos; la app las vuelve a aplicar sola (o reanuda el ciclo) cuando se reconectan.

## Uso

- Ejecuta `CentroLED.exe`. Al cerrar la ventana sigue en segundo plano en la bandeja del sistema. Clic derecho en el icono → Salir.
- **Teclado**: pestaña **Teclas** para el efecto o para pintar teclas (clic o arrastrar; clic derecho copia el color de una tecla); pestaña **Borde** para el efecto y color de la tira de luz. La vista previa se anima con el efecto elegido (es aproximada). Pulsa **Aplicar** para guardarlo en el teclado.
- **Mando**: elige "Ciclo suave", añade colores desde la paleta (clic en un color de la lista para quitarlo) y ajusta brillo y velocidad.
- **Batería en la barra de tareas**: cada dispositivo con batería tiene su icono junto al reloj: el dibujo del tipo de dispositivo (mando, cascos, ratón, teclado…) sobre una pila con el porcentaje (verde/ámbar/rojo, contorno amarillo al cargar, gris si es el último valor conocido). Se adapta al tema claro u oscuro de la barra. Se activa o desactiva en la barra lateral. Windows 11 coloca los iconos nuevos tras la flecha **^**: la app los marca para mostrarse y Windows lo aplica al reiniciar la sesión; o arrástralos una vez de la flecha a la barra.
- **Aviso de batería baja**: notificación de Windows al bajar del 15 % sin cargar (requiere tener las notificaciones de Windows activadas).
- "Iniciar con Windows" la arranca minimizada en la bandeja (si mueves el `.exe`, la entrada se corrige sola al abrirlo).
- La batería se actualiza cada minuto y al conectar/desconectar dispositivos.
- Configuración guardada en `%APPDATA%\CentroLED\config.txt`; si algo falla, el motivo queda en `%APPDATA%\CentroLED\error.txt`.

Línea de comandos (para scripts):

```
CentroLED.exe --probe salida.txt
CentroLED.exe --set <clave> <modo|off> <rrggbb> <brillo 0-100> <velocidad 0-100> salida.txt
```

## Límites del teclado

El firmware del Diablo sólo ejecuta un efecto a la vez en las teclas (y otro en el borde) y sus efectos multicolor usan una lista de colores fija: no admite ciclos con colores elegidos ni segundo color. Cada cambio se graba en la memoria del teclado (~1 s), así que no se puede animar desde el PC.

## Notas

- Si usas DS4Windows con HidHide, añade `CentroLED.exe` a la lista de aplicaciones permitidas de HidHide para que vea el mando.
- Logitech G HUB puede sobrescribir las luces de los G733 si está abierto.
- El protocolo del teclado se obtuvo capturando el software oficial de Tempest.

## Descarga y actualizaciones

En la [última release](https://github.com/Darwin2996/CentroLED/releases/latest) hay tres archivos:

| Archivo | Para qué |
|---|---|
| `CentroLED-Setup.exe` | Instalador ligero: descarga la última versión, la instala para tu usuario (sin permisos de administrador) en `%LOCALAPPDATA%\Programs\CentroLED`, crea accesos directos y aparece en *Aplicaciones instaladas* para desinstalarla |
| `CentroLED-Portable.exe` | Versión portable: guarda la configuración en una carpeta `CentroLED-datos` junto al ejecutable (ideal para un USB). También funciona si dejas un `portable.txt` junto a cualquier `CentroLED.exe` |
| `CentroLED.exe` | El programa tal cual, sin instalar (configuración en `%APPDATA%\CentroLED`) |

La app comprueba sola si hay una versión nueva (al arrancar y cada 6 horas). Si la hay, bajo el título aparece el botón **Actualizar a vX**: descarga la versión nueva, sustituye el ejecutable y se reinicia. También puedes pulsar la línea de versión para comprobarlo en el momento.

Para publicar una versión nueva: sube `version` en `Cargo.toml`, compila y crea una release con la etiqueta `vX.Y.Z` adjuntando `CentroLED.exe`, `CentroLED-Portable.exe` (copia del anterior) y `CentroLED-Setup.exe`.

## Compilar

```
cargo build --release
```
