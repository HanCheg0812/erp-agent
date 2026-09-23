use super::{lifecycle::operate, provider::configured};
use crate::{auth::Principal, platform::*};
use std::sync::Arc;
pub async fn sweep(app: App, stop: Arc<tokio_util::sync::CancellationToken>) {
    loop {
        tokio::select! {_=stop.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_secs(30))=>{}}
        if !configured() {
            continue;
        }
        if let Ok(db) = app.db.get().await {
            if let Ok(rows)=db.query("SELECT id,tenant_id,case_id,actor_id FROM case_sandboxes WHERE state<>'released' AND (expires_at<=now() OR state='releasing') ORDER BY expires_at LIMIT 20",&[]).await{
                drop(db);
                for r in rows{if stop.is_cancelled(){break}let actor=Principal{tenant_id:r.get(1),user_id:r.get(3),roles:vec!["planner".into()],email:String::new()};let _=operate(&app,&actor,r.get(2),"release",Some(r.get(0))).await;}
            }
        }
    }
}
