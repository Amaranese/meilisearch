mod error;

use std::marker::PhantomData;
use std::ops::Deref;

use actix_web::FromRequest;
use futures::future::err;
use futures::future::{ok, Ready};
use meilisearch_error::ResponseError;

use error::AuthenticationError;
use meilisearch_auth::{AuthController, AuthFilter};

pub struct GuardedData<T, D> {
    data: D,
    filters: AuthFilter,
    _marker: PhantomData<T>,
}

impl<T, D> GuardedData<T, D> {
    pub fn filters(&self) -> &AuthFilter {
        &self.filters
    }
}

impl<T, D> Deref for GuardedData<T, D> {
    type Target = D;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<P: Policy + 'static, D: 'static + Clone> FromRequest for GuardedData<P, D> {
    type Error = ResponseError;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        match req.app_data::<AuthController>().cloned() {
            Some(auth) => match req
                .headers()
                .get("Authorization")
                .map(|type_token| type_token.to_str().unwrap_or_default().splitn(2, ' '))
            {
                Some(mut type_token) => match type_token.next() {
                    Some("Bearer") => {
                        // TODO: find a less hardcoded way?
                        let index = req.match_info().get("index_uid");
                        match type_token.next() {
                            Some(token) => match P::authenticate(auth, token, index) {
                                Some(filters) => match req.app_data::<D>().cloned() {
                                    Some(data) => ok(Self {
                                        data,
                                        filters,
                                        _marker: PhantomData,
                                    }),
                                    None => err(AuthenticationError::IrretrievableState.into()),
                                },
                                None => {
                                    let token = token.to_string();
                                    err(AuthenticationError::InvalidToken(token).into())
                                }
                            },
                            None => {
                                err(AuthenticationError::InvalidToken("unknown".to_string()).into())
                            }
                        }
                    }
                    _otherwise => err(AuthenticationError::MissingAuthorizationHeader.into()),
                },
                None => match P::authenticate(auth, "", None) {
                    Some(filters) => match req.app_data::<D>().cloned() {
                        Some(data) => ok(Self {
                            data,
                            filters,
                            _marker: PhantomData,
                        }),
                        None => err(AuthenticationError::IrretrievableState.into()),
                    },
                    None => err(AuthenticationError::MissingAuthorizationHeader.into()),
                },
            },
            None => err(AuthenticationError::IrretrievableState.into()),
        }
    }
}

pub trait Policy {
    fn authenticate(auth: AuthController, token: &str, index: Option<&str>) -> Option<AuthFilter>;
}

pub mod policies {
    use chrono::Utc;
    use jsonwebtoken::{dangerous_insecure_decode, decode, Algorithm, DecodingKey, Validation};
    use once_cell::sync::Lazy;
    use serde::{Deserialize, Serialize};

    use crate::extractors::authentication::Policy;
    use meilisearch_auth::{Action, AuthController, AuthFilter, SearchRules};
    // reexport actions in policies in order to be used in routes configuration.
    pub use meilisearch_auth::actions;

    pub static TENANT_TOKEN_VALIDATION: Lazy<Validation> = Lazy::new(|| Validation {
        validate_exp: false,
        algorithms: vec![Algorithm::HS256, Algorithm::HS384, Algorithm::HS512],
        ..Default::default()
    });

    pub struct MasterPolicy;

    impl Policy for MasterPolicy {
        fn authenticate(
            auth: AuthController,
            token: &str,
            _index: Option<&str>,
        ) -> Option<AuthFilter> {
            if let Some(master_key) = auth.get_master_key() {
                if master_key == token {
                    return Some(AuthFilter::default());
                }
            }

            None
        }
    }

    pub struct ActionPolicy<const A: u8>;

    impl<const A: u8> Policy for ActionPolicy<A> {
        fn authenticate(
            auth: AuthController,
            token: &str,
            index: Option<&str>,
        ) -> Option<AuthFilter> {
            // authenticate if token is the master key.
            if auth.get_master_key().map_or(true, |mk| mk == token) {
                return Some(AuthFilter::default());
            }

            // Tenant token
            if let Some(filters) = ActionPolicy::<A>::authenticate_tenant_token(&auth, token, index)
            {
                return Some(filters);
            } else if let Some(action) = Action::from_repr(A) {
                // API key
                if let Ok(true) = auth.authenticate(token.as_bytes(), action, index) {
                    return auth.get_key_filters(token, None).ok();
                }
            }

            None
        }
    }

    impl<const A: u8> ActionPolicy<A> {
        fn authenticate_tenant_token(
            auth: &AuthController,
            token: &str,
            index: Option<&str>,
        ) -> Option<AuthFilter> {
            // Only search action can be accessed by a tenant token.
            if A != actions::SEARCH {
                return None;
            }

            // get token fields without validating it.
            let Claims {
                search_rules,
                exp,
                api_key_prefix,
            } = dangerous_insecure_decode::<Claims>(token).ok()?.claims;

            // Check index access if an index restriction is provided.
            if let Some(index) = index {
                if !search_rules.is_index_authorized(index) {
                    return None;
                }
            }

            // Check if token is expired.
            if let Some(exp) = exp {
                if Utc::now().timestamp() > exp {
                    return None;
                }
            }

            // check if parent key is authorized to do the action.
            if auth
                .is_key_authorized(api_key_prefix.as_bytes(), Action::Search, index)
                .ok()?
            {
                // Check if tenant token is valid.
                let key = auth.generate_key(&api_key_prefix)?;
                decode::<Claims>(
                    token,
                    &DecodingKey::from_secret(key.as_bytes()),
                    &TENANT_TOKEN_VALIDATION,
                )
                .ok()?;

                return auth
                    .get_key_filters(api_key_prefix, Some(search_rules))
                    .ok();
            }

            None
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Claims {
        search_rules: SearchRules,
        exp: Option<i64>,
        api_key_prefix: String,
    }
}
