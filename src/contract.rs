#[cfg(not(feature = "library"))]
use cosmwasm_std::{
    attr, entry_point, to_binary, Addr, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, Uint128, WasmMsg, StdError
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InfoResponse, InstantiateMsg, QueryMsg};
use crate::state::{State, STATE};
use std::time::{SystemTime, UNIX_EPOCH};

use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cosmwasm-16";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    if msg.start_time < _env.block.time.seconds() {
        return Err(ContractError::Std(StdError::generic_err(
            "start_time is less then current time",
        )));
    }

    if msg.end_time <= msg.start_time {
        return Err(ContractError::Std(StdError::generic_err(
            "end_time is less then or same as start_time",
        )));
    }

    let state = State {
        owner: info.sender,
        cw20_address: msg.cw20_address,
        price: Coin {
            denom: msg.denom,
            amount: msg.price,
        },
        balance: Uint128::zero(),
        start_time: msg.start_time,
        end_time: msg.end_time,
    };

    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SetPrice { denom, price } => try_set_price(
            deps,
            info.sender,
            Coin {
                denom,
                amount: price,
            },
        ),
        ExecuteMsg::Receive(msg) => try_receive(deps, msg),
        ExecuteMsg::Buy { denom, price } => try_buy(deps, info, denom, price),
        ExecuteMsg::WithdrawAll {} => try_withdraw_all(deps, info.sender),
    }
}
pub fn try_set_price(deps: DepsMut, sender: Addr, price: Coin) -> Result<Response, ContractError> {
    if STATE.load(deps.storage)?.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.price = price;
        Ok(state)
    })?;

    Ok(Response::default())
}

pub fn try_receive(deps: DepsMut, msg: Cw20ReceiveMsg) -> Result<Response, ContractError> {
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.balance += msg.amount;
        Ok(state)
    })?;

    Ok(Response::default())
}

pub fn try_buy(
    deps: DepsMut,
    info: MessageInfo,
    denom: String,
    price: Uint128,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage).unwrap();

    if denom != state.price.denom || price != state.price.amount {
        return Err(ContractError::PriceNotCurrentError {
            denom_current: state.price.denom,
            denom_provided: denom,
            price_current: state.price.amount,
            price_provided: price,
        });
    }

    let mut funds = Coin {
        amount: Uint128::zero(),
        denom: state.price.denom.clone(),
    };

    for coin in &info.funds {
        if coin.denom == state.price.denom {
            funds = Coin {
                amount: funds.amount + coin.amount,
                denom: funds.denom,
            }
        }
    }

    if funds.amount == Uint128::zero() {
        return Err(ContractError::IncorretFunds {});
    }

    let amount = match funds.amount.checked_div(state.price.amount) {
        Ok(r) => r,
        Err(_) => return Err(ContractError::DivideByZeroError {}),
    };

    // create transfer cw20 msg
    let transfer_cw20_msg = Cw20ExecuteMsg::Transfer {
        recipient: info.sender.into(),
        amount,
    };
    let exec_cw20_transfer = WasmMsg::Execute {
        contract_addr: state.cw20_address.into(),
        msg: to_binary(&transfer_cw20_msg)?,
        funds: vec![],
    };
    let cw20_transfer_cosmos_msg: CosmosMsg = exec_cw20_transfer.into();

    let transfer_bank_msg = cosmwasm_std::BankMsg::Send {
        to_address: state.owner.into(),
        amount: info.funds,
    };

    let transfer_bank_cosmos_msg: CosmosMsg = transfer_bank_msg.into();

    let updated_balance = match state.balance.checked_sub(amount) {
        Ok(r) => r,
        Err(_) => return Err(ContractError::SubtractionError {}),
    };

    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.balance = updated_balance;
        Ok(state)
    })?;

    Ok(Response::new()
           .add_messages(vec![cw20_transfer_cosmos_msg, transfer_bank_cosmos_msg])
           .add_attributes(vec![attr("amount", amount)])
    )
}

pub fn try_withdraw_all(deps: DepsMut, sender: Addr) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage).unwrap();

    if state.owner != sender {
        return Err(ContractError::Unauthorized {});
    }

    // create transfer cw20 msg
    let transfer_cw20_msg = Cw20ExecuteMsg::Transfer {
        recipient: state.owner.into(),
        amount: state.balance,
    };
    let exec_cw20_transfer = WasmMsg::Execute {
        contract_addr: state.cw20_address.into(),
        msg: to_binary(&transfer_cw20_msg)?,
        funds: vec![],
    };
    let cw20_transfer_cosmos_msg: CosmosMsg = exec_cw20_transfer.into();

    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.balance = Uint128::zero();
        Ok(state)
    })?;

    Ok(Response::new()
        .add_messages(vec![cw20_transfer_cosmos_msg])
        .add_attributes(vec![attr("amount", state.balance)])
    )
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetInfo {} => to_binary(&query_info(deps)?),
    }
}

fn query_info(deps: Deps) -> StdResult<InfoResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(InfoResponse {
        owner: state.owner,
        cw20_address: state.cw20_address,
        price: state.price,
        balance: state.balance,
    })
}


#[cfg(test)]
mod tests {
  use super::*;
      use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
      use cosmwasm_std::{coins, from_binary, Uint128};

      #[test]
      fn proper_initialization() {
          let mut deps = mock_dependencies(&[]);

          // set the start_time and end_time
          let start_time = SystemTime::now()
              .duration_since(UNIX_EPOCH)
              .unwrap()
              .as_secs();
          let end_time = start_time + 1000;

          let msg = InstantiateMsg {
              cw20_address: Addr::unchecked("asdf"),
              price: Uint128::from(7u128),
              denom: "token".to_string(),
              start_time,
              end_time,
          };
          let info = mock_info("creator", &coins(1000, "earth"));

          // we can just call .unwrap() to assert this was a success
          let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
          assert_eq!(0, res.messages.len());

          // it worked, let's query the state
          let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
          let value: InfoResponse = from_binary(&res).unwrap();
          assert_eq!(Uint128::from(7u128), value.price.amount);
      }

       #[test]
          fn set_price() {
              let mut deps = mock_dependencies(&coins(2, "token"));

               // set the start_time and end_time
               let start_time = SystemTime::now()
                   .duration_since(UNIX_EPOCH)
                   .unwrap()
                   .as_secs();
               let end_time = start_time + 1000;

              let msg = InstantiateMsg {
                  cw20_address: Addr::unchecked("asdf"),
                  price: Uint128::from(7u128),
                  denom: "token".to_string(),
                  start_time,
                  end_time,
              };
              let info = mock_info("creator", &coins(2, "token"));
              let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

              // owner can change price
              let info = mock_info("creator", &coins(2, "token"));
              let msg = ExecuteMsg::SetPrice {
                  denom: "token".to_string(),
                  price: Uint128::from(2u128),
              };
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

              // check price
              let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
              let value: InfoResponse = from_binary(&res).unwrap();
              assert_eq!(Uint128::from(2u128), value.price.amount);

              // non-owner cannot change price
              let info = mock_info("imposter", &coins(2, "token"));
              let msg = ExecuteMsg::SetPrice {
                  price: Uint128::from(10u128),
                  denom: "token".to_string(),
              };
              let _res = execute(deps.as_mut(), mock_env(), info, msg);
              assert!(_res.is_err());

              // check price
              let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
              let value: InfoResponse = from_binary(&res).unwrap();
              assert_eq!(Uint128::from(2u128), value.price.amount);
          }

          #[test]
          fn receive_cw20_token() {
              let mut deps = mock_dependencies(&coins(2, "token"));

              // set the start_time and end_time
              let start_time = SystemTime::now()
                  .duration_since(UNIX_EPOCH)
                  .unwrap()
                  .as_secs();
              let end_time = start_time + 1000;

              let msg = InstantiateMsg {
                  cw20_address: Addr::unchecked("asdf"),
                  price: Uint128::from(7u128),
                  denom: "token".to_string(),
                  start_time,
                  end_time
              };
              let info = mock_info("creator", &coins(2, "token"));
              let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
              // owner can change price
              let info = mock_info("creator", &coins(2, "token"));
              let msg = ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                  amount: Uint128::new(10),
                  sender: "asdf".to_string(),
                  msg: to_binary("a").unwrap(),
              });
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

              // check balance
              let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
              let value: InfoResponse = from_binary(&res).unwrap();
              assert_eq!(Uint128::new(10), value.balance);
          }

          #[test]
          fn buy_token() {
              let mut deps = mock_dependencies(&coins(2, "token"));
              let price: Uint128 = Uint128::from(7u128);
              let denom: String = "utoken".to_string();

              // set the start_time and end_time
              let start_time = SystemTime::now()
                  .duration_since(UNIX_EPOCH)
                  .unwrap()
                  .as_secs();
              let end_time = start_time + 1000;

              let msg = InstantiateMsg {
                  cw20_address: Addr::unchecked("asdf"),
                  price,
                  denom: denom.clone(),
                  start_time,
                  end_time
              };

              let info = mock_info("creator", &coins(2, "utoken"));
              let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

              let info = mock_info("creator", &coins(2, "token"));
              let msg = ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                  amount: Uint128::new(4),
                  sender: "asdf".to_string(),
                  msg: to_binary("a").unwrap(),
              });
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

              // basic buy
              let msg = ExecuteMsg::Buy {
                  price: Uint128::new(7),
                  denom: denom.clone(),
              };
              let info = mock_info("buyer", &coins(14, "utoken"));
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
              assert_eq!(_res.attributes.first().unwrap(), &attr("amount", "2"));

              // over pay
              let msg = ExecuteMsg::Buy {
                  denom: denom.clone(),
                  price,
              };
              let info = mock_info("buyer", &coins(20, "utoken"));
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
              assert_eq!(_res.attributes.first().unwrap(), &attr("amount", "2"));

              // wrong denom
              let msg = ExecuteMsg::Buy {
                  denom: denom.clone(),
                  price,
              };
              let info = mock_info("buyer", &coins(2, "uwrong"));
              let _res = execute(deps.as_mut(), mock_env(), info, msg);
              assert!(_res.is_err());
          }
          //
          // #[test]
          // fn buy_token_with_multiple_coins() {
          //     let mut deps = mock_dependencies(&coins(2, "token"));
          //
          //     let price: Uint128 = Uint128::from(7u128);
          //     let denom: String = "utoken".to_string();
          //     let msg = InstantiateMsg {
          //         cw20_address: Addr::unchecked("asdf"),
          //         price,
          //         denom: denom.clone(),
          //     };
          //     let info = mock_info("creator", &coins(2, "utoken"));
          //     let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
          //
          //     let info = mock_info("creator", &coins(2, "token"));
          //     let msg = ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
          //         amount: Uint128(4),
          //         sender: "asdf".to_string(),
          //         msg: to_binary("a").unwrap(),
          //     });
          //     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
          //
          //     // buy with three types of funds
          //     let msg = ExecuteMsg::Buy { denom, price };
          //     let funds: [Coin; 3] = [
          //         Coin {
          //             amount: Uint128(7),
          //             denom: "utoken".to_string(),
          //         },
          //         Coin {
          //             amount: Uint128(7),
          //             denom: "utoken".to_string(),
          //         },
          //         Coin {
          //             amount: Uint128(7),
          //             denom: "ufake".to_string(),
          //         },
          //     ];
          //     let info = mock_info("buyer", &funds);
          //     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
          //     assert_eq!(_res.attributes.first().unwrap(), &attr("amount", 2));
          // }
          //
          #[test]
          fn withdraw_cw20_token() {
              let mut deps = mock_dependencies(&coins(2, "token"));

              // set the start_time and end_time
              let start_time = SystemTime::now()
                  .duration_since(UNIX_EPOCH)
                  .unwrap()
                  .as_secs();
              let end_time = start_time + 1000;

              let msg = InstantiateMsg {
                  cw20_address: Addr::unchecked("asdf"),
                  price: Uint128::from(7u128),
                  denom: "token".to_string(),
                  start_time,
                  end_time
              };
              let info = mock_info("creator", &coins(2, "token"));
              let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

              let info = mock_info("creator", &coins(2, "token"));
              let msg = ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                  amount: Uint128::new(10),
                  sender: "asdf".to_string(),
                  msg: to_binary("a").unwrap(),
              });
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

              // check balance
              let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
              let value: InfoResponse = from_binary(&res).unwrap();
              assert_eq!(Uint128::new(10), value.balance);

              let info = mock_info("creator", &coins(2, "token"));
              let msg = ExecuteMsg::WithdrawAll {};
              let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

              // check balance
              let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
              let value: InfoResponse = from_binary(&res).unwrap();
              assert_eq!(Uint128::zero(), value.balance);
          }
          //
          // #[test]
          // fn withdraw_cw20_token_only_creator() {
          //     let mut deps = mock_dependencies(&coins(2, "token"));
          //
          //     let msg = InstantiateMsg {
          //         cw20_address: Addr::unchecked("asdf"),
          //         price: Uint128::from(7u128),
          //         denom: "token".to_string(),
          //     };
          //     let info = mock_info("creator", &coins(2, "token"));
          //     let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
          //
          //     let info = mock_info("imposter", &coins(2, "token"));
          //
          //     let msg = ExecuteMsg::WithdrawAll {};
          //     let _res = execute(deps.as_mut(), mock_env(), info, msg);
          //     assert!(_res.is_err());
          // }
}
