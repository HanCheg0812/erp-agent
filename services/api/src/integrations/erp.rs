use super::{
    response::{response_json, select, unavailable},
    types::Call,
};
use crate::platform::*;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

fn authorized_customers(scopes: Vec<String>, customer: Option<&str>) -> Vec<String> {
    scopes
        .into_iter()
        .filter(|scope| customer.is_none_or(|value| value == scope))
        .collect()
}

fn list_limit(arguments: Value) -> Result<u32> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct List {
        limit: Option<u32>,
    }
    let limit = parse::<List>(arguments)?.limit.unwrap_or(10);
    if !(1..=20).contains(&limit) {
        return Err(Error::bad("Limit must be 1-20"));
    }
    Ok(limit)
}

fn sales_order(arguments: Value) -> Result<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        sales_order: String,
    }
    let value = parse::<Read>(arguments)?.sales_order;
    text(&value, 140)?;
    Ok(value)
}

pub(super) async fn erp(
    app: &App,
    call: &Call,
    scopes: Vec<String>,
    customer: Option<String>,
    part: Option<String>,
) -> Result<Value> {
    let token = std::env::var("ERP_API_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            unavailable(
                "erp_not_configured",
                "ERP integration credentials are not configured",
            )
        })?;
    let customers = authorized_customers(scopes, customer.as_deref());
    if customers.is_empty() {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "erp_scope_missing",
            "No ERP customer is granted to this user in the current case scope",
        ));
    }
    let origin = env("ERP_API_URL", "http://127.0.0.1:18080");
    let base = format!(
        "{}/api/resource/Sales%20Order",
        origin.trim_end_matches('/')
    );
    if call.tool == "list_erp_orders" {
        let limit = list_limit(call.arguments.clone())?;
        let mut filters = json!([["customer", "in", &customers]]);
        if let Some(part) = part {
            filters.as_array_mut().unwrap().push(json!([
                "Sales Order Item",
                "item_code",
                "=",
                part
            ]));
        }
        let value = response_json(
            app.http
                .get(&base)
                .header("Authorization", format!("token {token}"))
                .query(&[
                    (
                        "fields",
                        json!([
                            "name",
                            "customer",
                            "company",
                            "status",
                            "delivery_date",
                            "transaction_date",
                            "modified",
                            "total_qty",
                            "grand_total",
                            "currency"
                        ])
                        .to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("limit_page_length", limit.to_string()),
                    ("order_by", "modified desc".into()),
                ])
                .send()
                .await
                .map_err(anyhow::Error::from)?,
        )
        .await?;
        let orders: Vec<_> = value["data"]
            .as_array()
            .ok_or_else(|| unavailable("erp_invalid_result", "ERP returned an invalid order list"))?
            .iter()
            .filter(|o| {
                o["customer"]
                    .as_str()
                    .is_some_and(|c| customers.iter().any(|s| s == c))
            })
            .take(limit as usize)
            .map(|o| {
                select(
                    o,
                    &[
                        "name",
                        "customer",
                        "company",
                        "status",
                        "delivery_date",
                        "transaction_date",
                        "modified",
                        "total_qty",
                        "grand_total",
                        "currency",
                    ],
                )
            })
            .collect();
        return Ok(json!({"source":"ERPNext","live":true,"orders":orders}));
    }
    let sales_order = sales_order(call.arguments.clone())?;
    let mut url = reqwest::Url::parse(&base).map_err(anyhow::Error::from)?;
    url.path_segments_mut()
        .map_err(|_| Error::bad("Invalid ERP endpoint"))?
        .push(&sales_order);
    let value = response_json(
        app.http
            .get(url)
            .header("Authorization", format!("token {token}"))
            .send()
            .await
            .map_err(anyhow::Error::from)?,
    )
    .await?;
    let order = &value["data"];
    if !order["customer"]
        .as_str()
        .is_some_and(|c| customers.iter().any(|s| s == c))
    {
        return Err(Error::missing());
    }
    let items: Vec<_> = order["items"]
        .as_array()
        .ok_or_else(|| unavailable("erp_invalid_order", "ERP order has no item list"))?
        .iter()
        .filter(|i| part.as_ref().is_none_or(|p| i["item_code"] == *p))
        .map(|i| {
            select(
                i,
                &[
                    "name",
                    "item_code",
                    "item_name",
                    "qty",
                    "uom",
                    "delivery_date",
                    "rate",
                    "amount",
                    "delivered_qty",
                ],
            )
        })
        .collect();
    if items.is_empty() && part.is_some() {
        return Err(Error::missing());
    }
    let mut order = select(
        order,
        &[
            "name",
            "customer",
            "company",
            "status",
            "docstatus",
            "delivery_date",
            "transaction_date",
            "modified",
            "total_qty",
            "grand_total",
            "currency",
        ],
    );
    order["items"] = json!(items);
    Ok(json!({"source":"ERPNext","live":true,"order":order}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erp_arguments_and_customer_scope_are_bounded() {
        assert_eq!(
            authorized_customers(vec!["C1".into(), "C2".into()], None),
            vec!["C1", "C2"]
        );
        assert_eq!(
            authorized_customers(vec!["C1".into(), "C2".into()], Some("C2")),
            vec!["C2"]
        );
        assert!(authorized_customers(vec!["C1".into()], Some("C2")).is_empty());
        assert_eq!(list_limit(json!({})).unwrap(), 10);
        assert_eq!(list_limit(json!({"limit":20})).unwrap(), 20);
        for value in [
            json!({"limit":0}),
            json!({"limit":21}),
            json!({"extra":true}),
        ] {
            assert!(list_limit(value).is_err());
        }
        assert_eq!(sales_order(json!({"sales_order":"SO-1"})).unwrap(), "SO-1");
        for value in [
            json!({"sales_order":""}),
            json!({"sales_order":"x".repeat(141)}),
            json!({"sales_order":"SO-1","extra":true}),
        ] {
            assert!(sales_order(value).is_err());
        }
    }
}
