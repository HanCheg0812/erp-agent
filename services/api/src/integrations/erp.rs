use super::{
    response::{erp_response_json, select, unavailable},
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

pub(super) struct ErpAuth {
    pub(super) token: String,
    pub(super) origin: String,
    pub(super) erp_user_id: String,
}

fn source(order: &Value) -> Option<Value> {
    Some(json!({
        "doctype": "Sales Order",
        "name": order.get("name")?.as_str()?,
        "modified": order.get("modified").cloned().unwrap_or(Value::Null),
    }))
}

pub(super) fn result_envelope(tool: &str, raw: Value, trace_id: &str) -> Result<Value> {
    let as_of = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339();
    let (data, sources) = match tool {
        "list_erp_orders" => {
            let orders = raw
                .get("orders")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| unavailable("erp_invalid_result", "ERP order list is incomplete"))?;
            let sources = orders.iter().filter_map(source).collect::<Vec<_>>();
            (json!({"orders": orders}), sources)
        }
        "read_erp_order" => {
            let order = raw
                .get("order")
                .cloned()
                .filter(Value::is_object)
                .ok_or_else(|| {
                    unavailable("erp_invalid_result", "ERP order result is incomplete")
                })?;
            let sources = source(&order).into_iter().collect::<Vec<_>>();
            (json!({"order": order}), sources)
        }
        _ => return Err(Error::bad("Unknown ERP result tool")),
    };
    Ok(json!({
        "status": "ok",
        "as_of": as_of,
        "trace_id": trace_id,
        "sources": sources,
        "data": data,
        "warnings": [],
    }))
}

pub(super) async fn erp(
    app: &App,
    call: &Call,
    scopes: Vec<String>,
    customer: Option<String>,
    part: Option<String>,
    auth: ErpAuth,
) -> Result<Value> {
    let customers = authorized_customers(scopes, customer.as_deref());
    if customers.is_empty() {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "erp_scope_missing",
            "No ERP customer is granted to this user in the current case scope",
        ));
    }
    let base = format!(
        "{}/api/resource/Sales%20Order",
        auth.origin.trim_end_matches('/')
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
        let value = erp_response_json(
            app.http
                .get(&base)
                .timeout(std::time::Duration::from_secs(8))
                .header("Authorization", format!("token {}", auth.token))
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
    let value = erp_response_json(
        app.http
            .get(url)
            .timeout(std::time::Duration::from_secs(8))
            .header("Authorization", format!("token {}", auth.token))
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

    #[test]
    fn result_envelope_is_traceable_and_keeps_erp_data_nested() {
        let value = result_envelope(
            "list_erp_orders",
            json!({"orders":[{"name":"SO-1","modified":"2026-09-24T00:00:00Z"}]}),
            "trace-1",
        )
        .unwrap();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["trace_id"], "trace-1");
        assert_eq!(value["data"]["orders"][0]["name"], "SO-1");
        assert_eq!(value["sources"][0]["doctype"], "Sales Order");
        assert_eq!(value["sources"][0]["modified"], "2026-09-24T00:00:00Z");
        assert_eq!(value["warnings"], json!([]));
    }
}
