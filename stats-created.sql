select '- <@' || u.discord_user_id || '> - ' || count(*) || ' requests created'
  from request r
       inner join "user" u on r.created_by = u.id
 where r.created_at > '2024-06-17 00:00:00Z'
 group by u.discord_user_id
 order by count(*) desc;
