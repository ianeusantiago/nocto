//! Lê as notificações que os apps mandam para o Windows (UserNotificationListener) e
//! repassa as do WhatsApp. Grátis e local: não fala com o WhatsApp, só com o Windows.
//!
//! Tenta ser avisado por evento; se o Windows não deixar (apps sem pacote às vezes não
//! podem assinar), consulta a lista a cada 3 s, o que custa quase nada.

use nocto_core::notify::{Source, source_of};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

fn texts(n: &UserNotification) -> Option<Vec<String>> {
    let binding = n
        .Notification()
        .ok()?
        .Visual()
        .ok()?
        .GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?)
        .ok()?;
    let elements = binding.GetTextElements().ok()?;
    let mut out = Vec::new();
    for i in 0..elements.Size().unwrap_or(0) {
        if let Ok(t) = elements.GetAt(i).and_then(|e| e.Text()) {
            out.push(t.to_string());
        }
    }
    Some(out)
}

/// Lê a lista atual e chama `on_message` para cada notificação nova do WhatsApp.
fn scan(listener: &UserNotificationListener, seen: &Mutex<HashSet<u32>>, report: bool, on_message: &(dyn Fn(Source, Vec<String>) + Send + Sync)) {
    let Ok(list) = listener.GetNotificationsAsync(NotificationKinds::Toast).and_then(|op| op.join()) else {
        return;
    };
    let mut current = HashSet::new();
    let mut fresh = Vec::new();
    {
        let seen = seen.lock().unwrap();
        for i in 0..list.Size().unwrap_or(0) {
            let Ok(n) = list.GetAt(i) else { continue };
            let Ok(id) = n.Id() else { continue };
            current.insert(id);
            if !seen.contains(&id) {
                fresh.push(n);
            }
        }
    }
    *seen.lock().unwrap() = current;
    if !report {
        return;
    }
    for n in fresh {
        let app = n
            .AppInfo()
            .and_then(|a| a.DisplayInfo())
            .and_then(|d| d.DisplayName())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if let Some(t) = texts(&n) {
            if let Some(source) = source_of(&app, &t) {
                on_message(source, t);
            }
        }
    }
}

/// Começa a observar. `on_message` recebe os textos da notificação (quem mandou, mensagem…).
pub fn watch(on_message: impl Fn(Source, Vec<String>) + Send + Sync + 'static) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let Ok(listener) = UserNotificationListener::Current() else { return };
        let allowed = UserNotificationListenerAccessStatus::Allowed;
        if listener.GetAccessStatus().ok() != Some(allowed)
            && listener.RequestAccessAsync().and_then(|op| op.join()).ok() != Some(allowed)
        {
            return;
        }
        let on_message: Arc<dyn Fn(Source, Vec<String>) + Send + Sync> = Arc::new(on_message);
        let seen = Arc::new(Mutex::new(HashSet::new()));
        // As notificações que já estavam lá não viram aviso.
        scan(&listener, &seen, false, &*on_message);

        let subscribed = {
            let listener2 = listener.clone();
            let seen = seen.clone();
            let on_message = on_message.clone();
            listener
                .NotificationChanged(&TypedEventHandler::new(move |_, _| {
                    scan(&listener2, &seen, true, &*on_message);
                    Ok(())
                }))
                .is_ok()
        };
        if subscribed {
            loop {
                std::thread::park();
            }
        }
        loop {
            std::thread::sleep(Duration::from_secs(3));
            scan(&listener, &seen, true, &*on_message);
        }
    });
}
