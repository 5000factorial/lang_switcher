use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use atspi::connection::AccessibilityConnection;
use atspi::events::FocusEvents;
use atspi::events::ObjectEvents;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{EventProperties, ObjectRefOwned, State};
use futures_util::StreamExt;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::converter::{SelectedTextDecision, convert_text, detect_selection_direction};
use crate::keymap::{Layout, char_to_key};

#[derive(Debug, Clone)]
pub struct SelectionConversion {
    pub decision: SelectedTextDecision,
    pub converted_text: String,
}

#[derive(Clone, Debug)]
struct CachedObject {
    object: ObjectRefOwned,
    application_root: Option<ObjectRefOwned>,
    seen_at: Instant,
}

#[derive(Clone)]
pub struct AtspiBridge {
    connection: Arc<AccessibilityConnection>,
    focused: Arc<RwLock<Option<CachedObject>>>,
    selected: Arc<RwLock<Option<CachedObject>>>,
}

impl AtspiBridge {
    const CACHED_SEARCH_TIMEOUT: Duration = Duration::from_millis(250);
    const SELECTION_SIGNAL_TTL: Duration = Duration::from_millis(1200);

    pub async fn new() -> Result<Self> {
        let connection = Arc::new(AccessibilityConnection::new().await?);
        connection.register_event::<FocusEvents>().await?;
        connection.register_event::<ObjectEvents>().await?;

        let bridge = Self {
            connection: connection.clone(),
            focused: Arc::new(RwLock::new(None)),
            selected: Arc::new(RwLock::new(None)),
        };
        bridge.spawn_focus_listener(connection);
        Ok(bridge)
    }

    pub async fn try_convert_selection(
        &self,
        current_layout: Layout,
    ) -> Result<Option<SelectionConversion>> {
        match self.try_convert_selection_once(current_layout).await {
            Ok(result) => Ok(result),
            Err(error) => {
                let message = error.to_string();
                if message.contains("ServiceUnknown") || message.contains("UnknownObject") {
                    debug!("AT-SPI selection unavailable: {message}");
                } else {
                    warn!("AT-SPI selection conversion failed: {error:#}");
                }
                *self.focused.write().await = None;
                *self.selected.write().await = None;
                Ok(None)
            }
        }
    }

    pub async fn should_try_primary_selection(&self) -> bool {
        let Some(selected) = self.selected.read().await.clone() else {
            return false;
        };
        if selected.seen_at.elapsed() > Self::SELECTION_SIGNAL_TTL {
            return false;
        }

        let Some(focused) = self.focused.read().await.clone() else {
            return false;
        };

        cached_objects_related(&selected, &focused)
    }

    pub async fn clear_recent_text_selection(&self) {
        *self.selected.write().await = None;
    }

    async fn try_convert_selection_once(
        &self,
        current_layout: Layout,
    ) -> Result<Option<SelectionConversion>> {
        if let Some(selected) = self.selected.read().await.clone()
            && let Some(result) = self
                .try_convert_selection_from_cached(selected, current_layout)
                .await?
        {
            return Ok(Some(result));
        }

        if let Some(cached) = self.focused.read().await.clone()
            && let Some(result) = self
                .try_convert_selection_from_cached(cached, current_layout)
                .await?
        {
            return Ok(Some(result));
        }

        let mut candidates = Vec::new();
        if let Some(discovered) = self.discover_focused_object().await? {
            candidates.push(discovered);
        }
        let candidate_snapshot = candidates.clone();
        for candidate in candidate_snapshot {
            if let Some(application_root) = self.application_root_for_object(&candidate).await?
                && !candidates
                    .iter()
                    .any(|known| same_object_ref(known, &application_root))
            {
                candidates.push(application_root);
            }
        }
        for application_root in self.registry_application_roots().await? {
            if !candidates
                .iter()
                .any(|known| same_object_ref(known, &application_root))
            {
                candidates.push(application_root);
            }
        }

        for candidate in candidates {
            if let Some(result) = self
                .try_convert_selection_in_subtree(candidate, current_layout)
                .await?
            {
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    async fn try_convert_selection_from_cached(
        &self,
        cached: CachedObject,
        current_layout: Layout,
    ) -> Result<Option<SelectionConversion>> {
        if let Some(application_root) = cached.application_root.clone()
            && !same_object_ref(&application_root, &cached.object)
            && let Some(result) = self
                .search_subtree_with_timeout(application_root, current_layout)
                .await?
        {
            return Ok(Some(result));
        } else if let Some(application_root) = cached.application_root.clone()
            && !same_object_ref(&application_root, &cached.object)
        {
            // fall through to the cached object itself if the wider app search found nothing
        }

        self.search_subtree_with_timeout(cached.object, current_layout)
            .await
    }

    async fn search_subtree_with_timeout(
        &self,
        root_object: ObjectRefOwned,
        current_layout: Layout,
    ) -> Result<Option<SelectionConversion>> {
        match tokio::time::timeout(
            Self::CACHED_SEARCH_TIMEOUT,
            self.try_convert_selection_in_subtree(root_object, current_layout),
        )
        .await
        {
            Ok(Ok(Some(result))) => Ok(Some(result)),
            Ok(Ok(None)) => Ok(None),
            Ok(Err(_)) | Err(_) => Ok(None),
        }
    }

    async fn try_convert_selection_in_subtree(
        &self,
        root_object: ObjectRefOwned,
        current_layout: Layout,
    ) -> Result<Option<SelectionConversion>> {
        let connection = self.connection.connection();
        let mut stack = vec![(root_object, 0usize)];

        while let Some((object, depth)) = stack.pop() {
            if depth > 32 {
                continue;
            }

            let Some(bus_name) = object.name().map(|name| name.to_owned()) else {
                continue;
            };

            let builder = match AccessibleProxy::builder(connection)
                .destination(bus_name)
                .and_then(|builder| builder.path(object.path()))
            {
                Ok(builder) => builder,
                Err(_) => continue,
            };

            let accessible = match builder.build().await {
                Ok(accessible) => accessible,
                Err(_) => continue,
            };

            if let Some(result) = self
                .try_convert_selection_on_accessible(&accessible, current_layout)
                .await?
            {
                return Ok(Some(result));
            }

            let children = match accessible.get_children().await {
                Ok(children) => children,
                Err(_) => continue,
            };

            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }

        Ok(None)
    }

    async fn try_convert_selection_on_accessible(
        &self,
        accessible: &AccessibleProxy<'_>,
        current_layout: Layout,
    ) -> Result<Option<SelectionConversion>> {
        let state = match accessible.get_state().await {
            Ok(state) => state,
            Err(_) => return Ok(None),
        };
        if !state.contains(State::Editable) {
            return Ok(None);
        }

        let proxies = match accessible.proxies().await {
            Ok(proxies) => proxies,
            Err(_) => return Ok(None),
        };

        let text = match proxies.text().await {
            Ok(text) => text,
            Err(_) => return Ok(None),
        };

        let selections = match text.get_nselections().await {
            Ok(selections) => selections,
            Err(_) => return Ok(None),
        };
        if selections <= 0 {
            return Ok(None);
        }

        let (start, end) = match text.get_selection(0).await {
            Ok(selection) => selection,
            Err(_) => return Ok(None),
        };
        if start >= end {
            return Ok(None);
        }

        let selected = match text.get_text(start, end).await {
            Ok(selected) => selected,
            Err(_) => return Ok(None),
        };
        if selected.trim().is_empty() {
            return Ok(None);
        }

        let decision = detect_selection_direction(&selected, current_layout);
        let converted = convert_text(decision.as_direction(), &selected);
        if converted == selected {
            return Ok(None);
        }
        let target_layout = decision.target_layout();
        if !can_inject_text(target_layout, &converted) {
            return Ok(None);
        }
        Ok(Some(SelectionConversion {
            decision,
            converted_text: converted,
        }))
    }

    async fn discover_focused_object(&self) -> Result<Option<ObjectRefOwned>> {
        let connection = self.connection.connection();
        let root = match AccessibleProxy::new(connection).await {
            Ok(root) => root,
            Err(_) => return Ok(None),
        };
        let children = match root.get_children().await {
            Ok(children) => children,
            Err(_) => return Ok(None),
        };

        let mut stack: Vec<(ObjectRefOwned, usize)> =
            children.into_iter().map(|child| (child, 0usize)).collect();

        while let Some((object, depth)) = stack.pop() {
            if depth > 48 {
                continue;
            }

            let Some(bus_name) = object.name().map(|name| name.to_owned()) else {
                continue;
            };
            let builder = match AccessibleProxy::builder(connection)
                .destination(bus_name)
                .and_then(|builder| builder.path(object.path()))
            {
                Ok(builder) => builder,
                Err(_) => continue,
            };
            let accessible = match builder.build().await {
                Ok(accessible) => accessible,
                Err(_) => continue,
            };

            let state = match accessible.get_state().await {
                Ok(state) => state,
                Err(_) => continue,
            };
            if state.contains(State::Focused)
                && accessible_supports_text_selection(&accessible, &state).await
            {
                return Ok(Some(object));
            }

            let children = match accessible.get_children().await {
                Ok(children) => children,
                Err(_) => continue,
            };

            for child in children {
                stack.push((child, depth + 1));
            }
        }

        Ok(None)
    }

    async fn registry_application_roots(&self) -> Result<Vec<ObjectRefOwned>> {
        let connection = self.connection.connection();
        let root = match AccessibleProxy::new(connection).await {
            Ok(root) => root,
            Err(_) => return Ok(Vec::new()),
        };

        match root.get_children().await {
            Ok(children) => Ok(children),
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn application_root_for_object(
        &self,
        object: &ObjectRefOwned,
    ) -> Result<Option<ObjectRefOwned>> {
        let Some(bus_name) = object.name().map(|name| name.to_owned()) else {
            return Ok(None);
        };

        let connection = self.connection.connection();
        let builder = match AccessibleProxy::builder(connection)
            .destination(bus_name)
            .and_then(|builder| builder.path(object.path()))
        {
            Ok(builder) => builder,
            Err(_) => return Ok(None),
        };

        let accessible = match builder.build().await {
            Ok(accessible) => accessible,
            Err(_) => return Ok(None),
        };

        match accessible.get_application().await {
            Ok(application) => Ok(Some(application)),
            Err(_) => Ok(None),
        }
    }

    fn spawn_focus_listener(&self, connection: Arc<AccessibilityConnection>) {
        let focused = self.focused.clone();
        let selected = self.selected.clone();
        tokio::spawn(async move {
            let stream = connection.event_stream();
            tokio::pin!(stream);

            while let Some(event) = stream.next().await {
                match event {
                    Ok(event) => {
                        if let Ok(focus) = FocusEvents::try_from(event.clone()) {
                            let object_ref: ObjectRefOwned = focus.object_ref().into();
                            if is_text_focus_candidate(connection.as_ref(), &object_ref).await {
                                let cached =
                                    build_cached_object(connection.as_ref(), object_ref).await;
                                if let Some(selected_hint) = selected.read().await.clone()
                                    && !cached_objects_related(&selected_hint, &cached)
                                {
                                    *selected.write().await = None;
                                }
                                *focused.write().await = Some(cached);
                            } else {
                                *focused.write().await = None;
                                *selected.write().await = None;
                            }
                        }

                        if let Ok(ObjectEvents::TextSelectionChanged(selection)) =
                            ObjectEvents::try_from(event)
                        {
                            let object_ref: ObjectRefOwned = selection.object_ref().into();
                            if is_text_focus_candidate(connection.as_ref(), &object_ref).await {
                                let cached =
                                    build_cached_object(connection.as_ref(), object_ref).await;
                                *focused.write().await = Some(cached.clone());
                                *selected.write().await = Some(cached);
                            } else {
                                *selected.write().await = None;
                            }
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if !message.contains("Missing interface") {
                            warn!("AT-SPI event stream error: {message}");
                        }
                    }
                }
            }
        });
    }
}

fn same_object_ref(left: &ObjectRefOwned, right: &ObjectRefOwned) -> bool {
    left.path_as_str() == right.path_as_str() && left.name_as_str() == right.name_as_str()
}

fn cached_objects_related(left: &CachedObject, right: &CachedObject) -> bool {
    same_object_ref(&left.object, &right.object)
        || left
            .application_root
            .as_ref()
            .zip(right.application_root.as_ref())
            .is_some_and(|(left_root, right_root)| same_object_ref(left_root, right_root))
}

fn can_inject_text(layout: Layout, text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, '\n' | '\t') || char_to_key(layout, ch).is_some())
}

async fn accessible_supports_text_selection(
    accessible: &AccessibleProxy<'_>,
    state: &atspi::StateSet,
) -> bool {
    if state.contains(State::Editable) || state.contains(State::SelectableText) {
        return true;
    }

    match accessible.proxies().await {
        Ok(proxies) => proxies.text().await.is_ok(),
        Err(_) => false,
    }
}

async fn is_text_focus_candidate(
    connection: &AccessibilityConnection,
    object: &ObjectRefOwned,
) -> bool {
    let Some(bus_name) = object.name().map(|name| name.to_owned()) else {
        return false;
    };

    let builder = match AccessibleProxy::builder(connection.connection())
        .destination(bus_name)
        .and_then(|builder| builder.path(object.path()))
    {
        Ok(builder) => builder,
        Err(_) => return false,
    };
    let accessible = match builder.build().await {
        Ok(accessible) => accessible,
        Err(_) => return false,
    };
    let state = match accessible.get_state().await {
        Ok(state) => state,
        Err(_) => return false,
    };

    accessible_supports_text_selection(&accessible, &state).await
}

async fn build_cached_object(
    connection: &AccessibilityConnection,
    object: ObjectRefOwned,
) -> CachedObject {
    let application_root = application_root_for_connection_object(connection, &object)
        .await
        .unwrap_or(None);
    CachedObject {
        object,
        application_root,
        seen_at: Instant::now(),
    }
}

async fn application_root_for_connection_object(
    connection: &AccessibilityConnection,
    object: &ObjectRefOwned,
) -> Result<Option<ObjectRefOwned>> {
    let Some(bus_name) = object.name().map(|name| name.to_owned()) else {
        return Ok(None);
    };

    let builder = match AccessibleProxy::builder(connection.connection())
        .destination(bus_name)
        .and_then(|builder| builder.path(object.path()))
    {
        Ok(builder) => builder,
        Err(_) => return Ok(None),
    };
    let accessible = match builder.build().await {
        Ok(accessible) => accessible,
        Err(_) => return Ok(None),
    };

    match accessible.get_application().await {
        Ok(application) => Ok(Some(application)),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::can_inject_text;
    use crate::keymap::Layout;

    #[test]
    fn validates_injectable_ascii_text() {
        assert!(can_inject_text(Layout::Us, "hello world"));
    }

    #[test]
    fn validates_injectable_cyrillic_text() {
        assert!(can_inject_text(Layout::Ru, "привет\nмир"));
    }

    #[test]
    fn rejects_uninjectable_text() {
        assert!(!can_inject_text(Layout::Us, "hello🙂"));
    }
}
